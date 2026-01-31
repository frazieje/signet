use std::pin::Pin;
use envoy_types::pb::envoy::extensions::filters::http::transform::v3::body_transformation::TransformAction;
use futures_util::Stream;
use tonic::{transport::Server, IntoRequest, Request as TonicRequest, Response as TonicResponse, Status, Streaming};
use envoy_types::pb::envoy::service::ext_proc::v3::external_processor_server::{ExternalProcessor, ExternalProcessorServer};
use envoy_types::pb::envoy::service::ext_proc::v3::{ProcessingRequest, ProcessingResponse};
use http::{Request, Response};
use http_body_util::Full;

use httpsig_hyper::{prelude::*, *};

type SignatureName = String;

/// This includes the method of the request corresponding to the request (the second element)
const COVERED_COMPONENTS: &[&str] = &["@status", "\"@method\";req", "date", "content-type", "content-digest"];

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

struct SignetExternalProcessor {}

#[tonic::async_trait]
impl ExternalProcessor for SignetExternalProcessor {
    type ProcessStream = BoxStream<ProcessingResponse>;

    async fn process(&self, request: TonicRequest<Streaming<ProcessingRequest>>) -> Result<TonicResponse<Self::ProcessStream>, Status> {
        todo!()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
