use hyper::service::Service;
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type HttpClient = Client<hyper::client::HttpConnector>;

static NOTFOUND: &[u8] = b"Not Found";

#[tokio::main]
async fn main() {
    let in_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let out_addr: SocketAddr = ([127, 0, 0, 1], 3001).into();

    let client_main = Client::new();

    let server = Server::bind(&in_addr).serve(MakeSvc {
        target: out_addr,
        client: client_main,
    });

    println!("Listening on http://{}", in_addr);
    println!("Proxying on http://{}", out_addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

struct Svc {
    target: SocketAddr,
    client: HttpClient,
}

impl Service<Request<Body>> for Svc {
    type Response = Response<Body>;
    type Error = GenericError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        fn mk_response(s: String) -> Result<Response<Body>, GenericError> {
            Ok(Response::builder().body(Body::from(s)).unwrap())
        }

        let res = match (req.method(), req.uri().path()) {
            (&Method::GET, "/health") => mk_response(r#"{}"#.into()),
            (&Method::GET, "/proxy") => {
                let uri_string = format!(
                    "http://{}{}",
                    self.target,
                    req.uri()
                        .path_and_query()
                        .map(|x| x.as_str())
                        .unwrap_or("/")
                );
                let uri = uri_string.parse().unwrap();
                *req.uri_mut() = uri;
                let result = self.client.request(req);
                return Box::pin(async move { Ok(result.await?) });
            }
            _ => {
                // Return 404 not found response.
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(NOTFOUND.into())
                    .unwrap())
            }
        };

        Box::pin(async { res })
    }
}

struct MakeSvc {
    target: SocketAddr,
    client: HttpClient,
}

impl<T> Service<T> for MakeSvc {
    type Response = Svc;
    type Error = GenericError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let target = self.target.clone();
        let client = self.client.clone();
        let fut = async move { Ok(Svc { target, client }) };
        Box::pin(fut)
    }
}
