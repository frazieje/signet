use std::pin::Pin;
use tonic::{transport::Server, IntoRequest, Request as TonicRequest, Response as TonicResponse, Status, Streaming};
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use envoy_types::pb::envoy::service::ext_proc::v3::external_processor_server::{ExternalProcessor, ExternalProcessorServer};
use envoy_types::pb::envoy::service::ext_proc::v3::{processing_request, ProcessingRequest, processing_response, ProcessingResponse, HeadersResponse, CommonResponse, BodyResponse, TrailersResponse};

use http::{Request, Response};
use http_body_util::Full;

use httpsig_hyper::{prelude::*, *};

type SignatureName = String;

/// This includes the method of the request corresponding to the request (the second element)
const COVERED_COMPONENTS: &[&str] = &["@status", "\"@method\";req", "date", "content-type", "content-digest"];

fn processing_response(response_type: processing_response::Response) -> ProcessingResponse {
    ProcessingResponse {
        dynamic_metadata: None,
        mode_override: None,
        request_drain: false,
        override_message_timeout: None,
        response: Some(response_type),
    }
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Debug, Default)]
struct SignetExternalProcessor {}

#[tonic::async_trait]
impl ExternalProcessor for SignetExternalProcessor {
    type ProcessStream = BoxStream<ProcessingResponse>;

    async fn process(&self, request: TonicRequest<Streaming<ProcessingRequest>>) -> Result<TonicResponse<Self::ProcessStream>, Status> {

        let mut stream = request.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            while let Ok(Some(req)) = stream.message().await {
                let Some(message) = req.request else {
                    continue;
                };
                let resp = match message {
                    processing_request::Request::RequestHeaders(_) => {
                        println!("received RequestHeaders");
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
                    processing_request::Request::ResponseHeaders(_) => {
                        println!("received ResponseHeaders");
                        processing_response(
                            processing_response::Response::ResponseHeaders(
                                HeadersResponse { response: Some(CommonResponse::default()) }
                            )
                        )
                    },
                    processing_request::Request::ResponseBody(_) => {
                        println!("received ResponseBody");
                        processing_response(
                            processing_response::Response::ResponseBody(
                                BodyResponse { response: Some(CommonResponse::default()) }
                            )
                        )
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

        Ok(TonicResponse::new(Box::pin(ReceiverStream::new(rx)) as Self::ProcessStream))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let signet = SignetExternalProcessor::default();

    Server::builder()
        .add_service(ExternalProcessorServer::new(signet))
        .serve(addr)
        .await?;

    Ok(())
}
