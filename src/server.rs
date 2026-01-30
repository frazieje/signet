pub mod hello_world;
use tonic::{transport::Server, IntoRequest, Request as TonicRequest, Response as TonicResponse, Status, Streaming};

use http::{Request, Response};
use http_body_util::Full;

use httpsig_hyper::{prelude::*, *};

type SignatureName = String;

/// This includes the method of the request corresponding to the request (the second element)
const COVERED_COMPONENTS: &[&str] = &["@status", "\"@method\";req", "date", "content-type", "content-digest"];

pub mod envoy {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/gen/envoy/service/ext_proc/v3/envoy.service.ext_proc.v3.rs"));
}

use envoy::external_processor_server::{ExternalProcessor, ExternalProcessorServer};
use crate::envoy::ProcessingRequest;

struct SignetExternalProcessor {}

#[tonic::async_trait]
impl ExternalProcessor for SignetExternalProcessor {
    type ProcessStream = ();

    async fn process(&self, request: TonicRequest<Streaming<ProcessingRequest>>) -> Result<TonicResponse<Self::ProcessStream>, Status> {
        todo!()
    }
}


#[derive(Debug, Default)]
pub struct MyGreeter {
    x: i32
}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: TonicRequest<HelloRequest>, // Accept request of type HelloRequest
    ) -> Result<TonicResponse<HelloReply>, Status> { // Return an instance of type HelloReply
        println!("Got a request: {:?}", request);

        let reply = HelloReply {
            message: format!("Hello {}!", request.into_inner().name), // We must use .into_inner() as the fields of gRPC requests and responses are private
        };

        Ok(TonicResponse::new(reply)) // Send back our formatted greeting
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let greeter = MyGreeter::default();

    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
