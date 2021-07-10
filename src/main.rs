use hyper::{
    service::Service,
    {Body, Client, Method, Request, Response, Server, StatusCode},
};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type HttpClient = Client<hyper::client::HttpConnector>;
type Workers = Arc<WorkerManager>;

static NOTFOUND: &[u8] = b"Not Found";

#[tokio::main]
async fn main() {
    let in_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let target: SocketAddr = ([127, 0, 0, 1], 3001).into();
    let client = Client::new();
    let workers = WorkerManager::new();

    let server = Server::bind(&in_addr).serve(MakeSvc {
        target,
        client,
        workers,
    });

    println!("Listening on http://{}", in_addr);
    println!("Proxying on http://{}", target);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

struct WorkerManager {
    workers: Arc<Mutex<HashMap<String, bool>>>,
}
impl WorkerManager {
    pub fn new() -> Arc<Self> {
        let m = WorkerManager {
            workers: Default::default(),
        };
        Arc::new(m)
    }

    pub fn get(&self, key: &str) -> bool {
        let mut map = self.workers.lock().unwrap();
        // Return a clone if the value is a complex type.
        if let Some(&value) = map.get(key) {
            value
        } else {
            println!("Creating {}", key);
            let value = true;
            map.insert(key.into(), value);
            value
        }
    }
}

struct Svc {
    target: SocketAddr,
    client: HttpClient,
    workers: Workers,
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

        if req.method() == &Method::GET && req.uri().path() == "/health" {
            return Box::pin(async { mk_response(r#"{}"#.into()) });
        }

        // "/proxy/{routeName}"
        if req.uri().path().starts_with("/proxy/") {
            let route_name = req.uri().path().strip_prefix("/proxy/").unwrap_or("");
            println!("Receive {}", route_name);

            let r = self.workers.get(route_name).clone();
            if r {
                println!("Found route")
            }

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

        Box::pin(async {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(NOTFOUND.into())
                .unwrap())
        })
    }
}

struct MakeSvc {
    target: SocketAddr,
    client: HttpClient,
    workers: Workers,
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
        let workers = self.workers.clone();
        Box::pin(async move {
            Ok(Svc {
                target,
                client,
                workers,
            })
        })
    }
}
