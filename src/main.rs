pub use anyhow::anyhow;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type HttpClient = Client<hyper::client::HttpConnector>;
type Workers = Arc<WorkerManager>;
/// A generic wrapper that can encapsulate any concrete error type.
pub type AnyError = anyhow::Error;

static NOTFOUND: &[u8] = b"Not Found";

#[tokio::main]
async fn main() {
    let in_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let target: SocketAddr = ([127, 0, 0, 1], 3001).into();
    let client = Client::new();
    let workers = WorkerManager::new();

    let rp = std::sync::Arc::new(Svc {
        target,
        client,
        workers,
    });

    let make_svc = make_service_fn(move |_conn| {
        let rp = rp.clone();
        async {
            Ok::<_, GenericError>(service_fn(move |req| {
                let rp = rp.clone();
                async move { rp.handle(req).await }
            }))
        }
    });

    let server = Server::bind(&in_addr).serve(make_svc);

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

    pub async fn get(&self, key: &str) -> Result<bool, AnyError> {
        let mut map = self.workers.lock().unwrap();
        // Return a clone if the value is a complex type.
        if let Some(&value) = map.get(key) {
            Ok(value)
        } else {
            println!("Creating {}", key);
            let value = true;
            map.insert(key.into(), value);
            Ok(value)
        }
    }
}

struct Svc {
    target: SocketAddr,
    client: HttpClient,
    workers: Workers,
}

impl Svc {
    async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, GenericError> {
        fn mk_response(s: String) -> Result<Response<Body>, GenericError> {
            Ok(Response::builder().body(Body::from(s)).unwrap())
        }
        if req.method() == &Method::GET && req.uri().path() == "/health" {
            return mk_response(r#"{}"#.into());
        }

        // "/proxy/{routeName}"
        if req.uri().path().starts_with("/proxy/") {
            let res = self.reverse_proxy(req).await?;
            return Ok(res);
        }

        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(NOTFOUND.into())
            .unwrap())
    }

    pub async fn reverse_proxy(
        &self,
        mut req: Request<Body>,
    ) -> Result<Response<Body>, GenericError> {
        let route_name = req.uri().path().strip_prefix("/proxy/").unwrap_or("");
        println!("Receive {}", route_name);
        let r = self.workers.get(route_name).await?;
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
        let response = self.client.request(req).await?;
        Ok(response)
    }
}
