
use tonic::{transport::Server, IntoRequest, Request as TonicRequest, Response as TonicResponse, Status};

use http::{Request, Response};
use http_body_util::Full;

use httpsig_hyper::{prelude::*, *};

type SignatureName = String;

/// This includes the method of the request corresponding to the request (the second element)
const COVERED_COMPONENTS: &[&str] = &["@status", "\"@method\";req", "date", "content-type", "content-digest"];

use hello_world::greeter_server::{Greeter, GreeterServer};
use hello_world::{HelloReply, HelloRequest};
pub mod hello_world {
    tonic::include_proto!("helloworld"); // The string specified here must match the proto package name
}

#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: TonicRequest<HelloRequest>, // Accept request of type HelloRequest
    ) -> Result<TonicResponse<HelloReply>, Status> { // Return an instance of type HelloReply
        println!("Got a request: {:?}", request);

        Request::from_parts();

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
