pub use anyhow::anyhow;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::sync::Mutex; // TODO: Should avoid holding a lock before await, although I want singleflight

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type HttpClient = Client<hyper::client::HttpConnector>;
type Workers = Arc<WorkerManager>;
/// A generic wrapper that can encapsulate any concrete error type.
pub type AnyError = anyhow::Error;

static NOTFOUND: &[u8] = b"Not Found";

#[tokio::main]
async fn main() {
    let in_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let client = Client::new();
    let workers = WorkerManager::new();

    let rp = std::sync::Arc::new(Svc { client, workers });

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

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

struct WorkerManager {
    workers: Arc<Mutex<HashMap<String, SocketAddr>>>,
}
impl WorkerManager {
    pub fn new() -> Arc<Self> {
        let m = WorkerManager {
            workers: Default::default(),
        };
        Arc::new(m)
    }

    pub async fn get(&self, key: &str) -> Result<SocketAddr, AnyError> {
        let mut map = self.workers.lock().await;
        // Return a clone if the value is a complex type.
        if let Some(&value) = map.get(key) {
            Ok(value)
        } else {
            let add = self.spawn(key).await?;
            println!("Got: {}", add);
            map.insert(key.into(), add);
            Ok(add)
        }
    }

    async fn spawn(&self, key: &str) -> Result<SocketAddr, AnyError> {
        println!("Creating {}", key);
        // Setup new thread
        async fn handle(_: Request<Body>) -> Result<Response<Body>, Infallible> {
            Ok(Response::new("Hello, World!".into()))
        }
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async {
            let make_svc =
                make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });
            let in_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let server = Server::bind(&in_addr).serve(make_svc);

            println!("Listening on http://{}", server.local_addr());

            tx.send(server.local_addr().clone()).unwrap();
            if let Err(e) = server.await {
                eprintln!("server error: {}", e);
            }
        });

        let value = rx.await?;
        Ok(value.clone())
    }
}

struct Svc {
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
        let target = self.workers.get(route_name).await?;
        let uri_string = format!(
            "http://{}{}",
            target,
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
