use std::pin::Pin;
use std::sync::Arc;
use envoy_types::ext_authz::v3::pb::{HeaderValue, HeaderValueOption};
use tonic::{transport::Server, Request as TonicRequest, Response as TonicResponse, Status, Streaming};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use envoy_types::pb::envoy::service::ext_proc::v3::external_processor_server::{ExternalProcessor, ExternalProcessorServer};
use envoy_types::pb::envoy::service::ext_proc::v3::{HttpHeaders, processing_request, ProcessingRequest, processing_response, ProcessingResponse, HeadersResponse, CommonResponse, BodyResponse, TrailersResponse, HeaderMutation, HttpBody};

use http::{Request, Response};
use http_body_util::Full;

use httpsig_hyper::{prelude::*, *};

/// This includes the method of the request corresponding to the request (the second element)
const COVERED_COMPONENTS: &[&str] = &["@status", /*"\"@method\";req",*/ "date", "content-type", "content-digest"];

use bytes::Bytes;
use http::{HeaderMap as HttpHeaderMap, HeaderName, StatusCode};
use sha2::{Digest, Sha256};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
#[derive(Debug)]
struct UpstreamResponseAcc {
    status: Option<StatusCode>,
    headers: HttpHeaderMap,
    body_digest: Sha256,
    done: bool
}

impl Default for UpstreamResponseAcc {
    fn default() -> Self {
        Self {
            status: None,
            headers: HttpHeaderMap::new(),
            body_digest: Sha256::new(),
            done: false
        }
    }
}

impl UpstreamResponseAcc {
    fn on_response_headers(&mut self, response_headers: HttpHeaders) {
        self.done = response_headers.end_of_stream;
        for header in response_headers.headers.unwrap_or_default().headers {

            if header.key == ":status" {
                let s = if !header.raw_value.is_empty() {
                    std::str::from_utf8(&header.raw_value).expect("malformed status")
                } else {
                    header.value.as_str()
                };
                if let Ok(code) = s.parse::<u16>() {
                    self.status = StatusCode::from_u16(code).ok();
                }
                continue;
            }
            if header.key.starts_with(':') {
                continue;
            }

            let Ok(key) = HeaderName::from_bytes(header.key.as_bytes()) else {
                continue;
            };
            let Ok(value) = http::header::HeaderValue::from_str(header.value.as_str()) else {
                continue;
            };
            self.headers.append(key, value);
        }
    }

    fn on_response_body_chunk(&mut self, chunk: HttpBody) {
        self.body_digest.update(&chunk.body);
        self.done = chunk.end_of_stream;
    }

    fn maybe_attach_date(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.done || self.headers.contains_key("date") {
            return Ok(());
        }
        // TODO: Add date header
        Ok(())
    }

    fn maybe_attach_content_digest(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.done || self.headers.contains_key("content-digest") {
            return Ok(());
        }
        let digest_bytes = self.body_digest.clone().finalize(); // clone so self stays usable if needed
        let b64 = B64.encode(digest_bytes);

        // RFC 9530 format: "sha-256=:<base64>:"
        let value_str = format!("sha-256=:{b64}:");

        self.headers.insert(
            HeaderName::from_static("content-digest"),
            http::header::HeaderValue::from_str(&value_str)?,
        );

        Ok(())
    }

    fn maybe_attach_headers(&mut self) {
        self.maybe_attach_content_digest().unwrap_or_default();
        self.maybe_attach_date().unwrap_or_default();
    }

    fn build_http_response(&self) -> Response<Full<Bytes>> {
        let mut builder = Response::builder()
            .status(self.status.unwrap_or(StatusCode::OK));

        for header in self.headers.iter() {
            builder = builder.header(header.0, header.1);
        }
        // Response::builder() wants headers via the builder, but it’s easiest to set them after.
        let resp = builder
            .body(Full::new(Bytes::new()))
            .expect("valid response");

        resp
    }
}

fn processing_response(response_type: processing_response::Response) -> ProcessingResponse {
    ProcessingResponse {
        dynamic_metadata: None,
        mode_override: None,
        request_drain: false,
        override_message_timeout: None,
        response: Some(response_type),
    }
}

trait AsProcessingResponse {
    fn to_processing_response(self) -> ProcessingResponse;
}

impl<T> AsProcessingResponse for Response<T> {
    fn to_processing_response(self) -> ProcessingResponse {
        let mut hvos: Vec<HeaderValueOption> = vec![];
        for header in self.headers().iter() {
            let mut hvo = HeaderValueOption::default();
            hvo.header = Some(
                HeaderValue {
                    key: header.0.to_string(),
                    value: "".to_string(),
                    raw_value: header.1.as_bytes().to_vec(),
                }
            );
            hvo.keep_empty_value = true;
            hvos.push(hvo);
        }
        processing_response(
            processing_response::Response::ResponseBody(
                BodyResponse {
                    response: Some(
                        CommonResponse {
                            status: self.status().as_u16().into(),
                            header_mutation: Some(
                                HeaderMutation {
                                    set_headers: hvos,
                                    remove_headers: vec![]
                                }
                            ),
                            body_mutation: None,
                            trailers: None,
                            clear_route_cache: false,
                        }
                    )
                }
            )
        )
    }
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Debug)]
struct SignetExternalProcessor {
    secret_key: Arc<SecretKey>
}

#[tonic::async_trait]
impl ExternalProcessor for SignetExternalProcessor {
    type ProcessStream = BoxStream<ProcessingResponse>;

    async fn process(&self, request: TonicRequest<Streaming<ProcessingRequest>>) -> Result<TonicResponse<Self::ProcessStream>, Status> {

        let mut stream = request.into_inner();
        let sk = self.secret_key.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            println!("Inside async move");
            let mut acc = UpstreamResponseAcc::default();
            while let Ok(Some(req)) = stream.message().await {
                println!("New stream message");
                let Some(message) = req.request else {
                    continue;
                };
                let resp = match message {
                    processing_request::Request::RequestHeaders(_) => {
                        println!("received RequestHeaders");
                        // TODO: Skip the rest of the ext_proc if signing header not present
                        processing_response(
                            processing_response::Response::RequestHeaders(
                                HeadersResponse { response: Some(CommonResponse::default()) }
                            )
                        )
                    },
                    processing_request::Request::RequestBody(_) => {
                        println!("received RequestBody");
                        processing_response(
                            processing_response::Response::RequestBody(
                                BodyResponse { response: Some(CommonResponse::default()) }
                            )
                        )
                    },
                    processing_request::Request::RequestTrailers(_) => {
                        println!("received RequestTrailers");
                        processing_response(
                            processing_response::Response::RequestTrailers(
                                TrailersResponse::default()
                            )
                        )
                    },
                    processing_request::Request::ResponseHeaders(response_headers) => {
                        println!("received ResponseHeaders");
                        acc.on_response_headers(response_headers);
                        // Maybe return a processing response that alters the body processing mode
                        acc.maybe_attach_headers();
                        processing_response(
                            processing_response::Response::ResponseHeaders(
                                HeadersResponse { response:
                                    Some(
                                        CommonResponse {
                                            status: 200,
                                            header_mutation: Some(
                                                HeaderMutation {
                                                    set_headers: vec![
                                                        HeaderValueOption {
                                                            header: Some(
                                                                HeaderValue {
                                                                    key: "X-EXT-PROC".to_string(),
                                                                    value: "".to_string(),
                                                                    raw_value: b"YAY".to_vec(),
                                                                }
                                                            ),
                                                            append: None,
                                                            append_action: 0,
                                                            keep_empty_value: false,
                                                        }
                                                    ],
                                                    remove_headers: vec![]
                                                }
                                            ),
                                            body_mutation: None,
                                            trailers: None,
                                            clear_route_cache: false,
                                        }
                                    )
                                }
                            )
                        )
                    },
                    processing_request::Request::ResponseBody(response_body) => {
                        println!("received ResponseBody");
                        acc.on_response_body_chunk(response_body);
                        acc.maybe_attach_headers();
                        let mut http_response = acc.build_http_response();
                        let covered_components = COVERED_COMPONENTS
                            .iter()
                            .map(|v| message_component::HttpMessageComponentId::try_from(*v))
                            .collect::<Result<Vec<_>, _>>()
                            .unwrap();
                        let signature_params = HttpSignatureParams::try_new(&covered_components).unwrap();

                        http_response.set_message_signature(
                            &signature_params,
                            sk.as_ref(),
                            Some("signature_name"),
                            None::<&Request<&str>>
                        ).await.expect("error signing");
                        http_response.to_processing_response()
                    },
                    processing_request::Request::ResponseTrailers(_) => {
                        println!("received ResponseTrailers");
                        processing_response(
                            processing_response::Response::ResponseTrailers(
                                TrailersResponse::default()
                            )
                        )
                    }
                };

                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });

        println!("Sending ok response");

        Ok(TonicResponse::new(Box::pin(ReceiverStream::new(rx)) as Self::ProcessStream))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::]:50051".parse()?;
    let secret_key_string = std::fs::read_to_string("key.pem").expect("Unable to read key.pem");
    let secret_key = SecretKey::from_pem(secret_key_string.as_str()).expect("Unable to read key.pem");
    let signet = SignetExternalProcessor {
        secret_key: Arc::new(secret_key),
    };

    Server::builder()
        .add_service(ExternalProcessorServer::new(signet))
        .serve(addr)
        .await?;

    Ok(())
}
