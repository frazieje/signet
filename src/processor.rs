use envoy_types::pb::envoy::config::core::v3::{HeaderValue, HeaderValueOption};
use envoy_types::pb::envoy::service::ext_proc::v3::external_processor_server::ExternalProcessor;
use envoy_types::pb::envoy::service::ext_proc::v3::{
    BodyResponse, CommonResponse, HeaderMutation, HeadersResponse, ProcessingRequest,
    ProcessingResponse, TrailersResponse, processing_request, processing_response,
};
use http::Response;
use httpsig_hyper::prelude::*;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request as TonicRequest, Response as TonicResponse, Status, Streaming};
use tracing::{error, info};

use crate::signing::UpstreamResponseAcc;

fn make_processing_response(
    response_type: processing_response::Response,
) -> ProcessingResponse {
    ProcessingResponse {
        dynamic_metadata: None,
        mode_override: None,
        request_drain: false,
        override_message_timeout: None,
        response: Some(response_type),
    }
}

pub trait AsProcessingResponse {
    fn to_common_response(self) -> CommonResponse;
}

impl<T> AsProcessingResponse for Response<T> {
    fn to_common_response(self) -> CommonResponse {
        let hvos: Vec<HeaderValueOption> = self
            .headers()
            .iter()
            .map(|(name, value)| HeaderValueOption {
                header: Some(HeaderValue {
                    key: name.to_string(),
                    value: "".to_string(),
                    raw_value: value.as_bytes().to_vec(),
                }),
                keep_empty_value: true,
                ..Default::default()
            })
            .collect();
        CommonResponse {
            status: self.status().as_u16().into(),
            header_mutation: Some(HeaderMutation {
                set_headers: hvos,
                remove_headers: vec![],
            }),
            body_mutation: None,
            trailers: None,
            clear_route_cache: false,
        }
    }
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Debug)]
pub struct SignetExternalProcessor {
    pub secret_key: Arc<SecretKey>,
}

#[tonic::async_trait]
impl ExternalProcessor for SignetExternalProcessor {
    type ProcessStream = BoxStream<ProcessingResponse>;

    async fn process(
        &self,
        request: TonicRequest<Streaming<ProcessingRequest>>,
    ) -> Result<TonicResponse<Self::ProcessStream>, Status> {
        let mut stream = request.into_inner();
        let sk = self.secret_key.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            let mut acc = UpstreamResponseAcc::default();
            while let Ok(Some(req)) = stream.message().await {
                let Some(message) = req.request else {
                    continue;
                };
                let resp = match message {
                    processing_request::Request::RequestHeaders(_) => {
                        info!("received RequestHeaders");
                        make_processing_response(
                            processing_response::Response::RequestHeaders(HeadersResponse {
                                response: Some(CommonResponse::default()),
                            }),
                        )
                    }
                    processing_request::Request::RequestBody(_) => {
                        info!("received RequestBody");
                        make_processing_response(
                            processing_response::Response::RequestBody(BodyResponse {
                                response: Some(CommonResponse::default()),
                            }),
                        )
                    }
                    processing_request::Request::RequestTrailers(_) => {
                        info!("received RequestTrailers");
                        make_processing_response(
                            processing_response::Response::RequestTrailers(
                                TrailersResponse::default(),
                            ),
                        )
                    }
                    processing_request::Request::ResponseHeaders(response_headers) => {
                        info!("received ResponseHeaders");
                        acc.on_response_headers(response_headers);
                        let finalized =
                            acc.maybe_finalize(&sk).await.unwrap_or_else(|e| {
                                error!("Finalization error: {e}");
                                None
                            });
                        make_processing_response(
                            processing_response::Response::ResponseHeaders(HeadersResponse {
                                response: finalized.map(|r| r.to_common_response()),
                            }),
                        )
                    }
                    processing_request::Request::ResponseBody(response_body) => {
                        info!("received ResponseBody");
                        acc.on_response_body_chunk(response_body);
                        let finalized =
                            acc.maybe_finalize(&sk).await.unwrap_or_else(|e| {
                                error!("Finalization error: {e}");
                                None
                            });
                        make_processing_response(
                            processing_response::Response::ResponseBody(BodyResponse {
                                response: finalized.map(|r| r.to_common_response()),
                            }),
                        )
                    }
                    processing_request::Request::ResponseTrailers(_) => {
                        info!("received ResponseTrailers");
                        make_processing_response(
                            processing_response::Response::ResponseTrailers(
                                TrailersResponse::default(),
                            ),
                        )
                    }
                };

                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });

        info!("Sending ok response");

        Ok(TonicResponse::new(
            Box::pin(ReceiverStream::new(rx)) as Self::ProcessStream
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;

    #[test]
    fn to_common_response_preserves_headers() {
        let response = http::Response::builder()
            .status(200)
            .header("x-custom", "value1")
            .header("content-type", "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let common = response.to_common_response();
        let mutation = common.header_mutation.unwrap();
        let headers: Vec<_> = mutation
            .set_headers
            .iter()
            .map(|hvo| {
                let h = hvo.header.as_ref().unwrap();
                (h.key.clone(), h.raw_value.clone())
            })
            .collect();
        assert!(headers
            .iter()
            .any(|(k, v)| k == "x-custom" && v == b"value1"));
        assert!(headers
            .iter()
            .any(|(k, v)| k == "content-type" && v == b"application/json"));
    }
}
