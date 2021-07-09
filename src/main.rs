use hyper::client::HttpConnector;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode};
use std::net::SocketAddr;

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, GenericError>;

static NOTFOUND: &[u8] = b"Not Found";

async fn client_request_response(
    client: &Client<HttpConnector>,
    mut req: Request<Body>,
) -> Result<Response<Body>> {
    let out_addr: SocketAddr = "127.0.0.1:3001".parse().unwrap();

    let uri_string = format!(
        "http://{}{}",
        out_addr,
        req.uri()
            .path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/")
    );
    let uri = uri_string.parse().unwrap();
    *req.uri_mut() = uri;
    let response = client.request(req).await?;
    Ok(response)
}

async fn response_examples(
    req: Request<Body>,
    client: Client<HttpConnector>,
) -> Result<Response<Body>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/health") => Ok(Response::new(r#"{}"#.into())),
        (&Method::GET, "/proxy") => client_request_response(&client, req).await,
        _ => {
            // Return 404 not found response.
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(NOTFOUND.into())
                .unwrap())
        }
    }
}

#[tokio::main]
async fn main() {
    // let in_addr = ([127, 0, 0, 1], 3001).into();
    let in_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let out_addr: SocketAddr = ([127, 0, 0, 1], 3001).into();

    let client_main = Client::new();

    // let out_addr_clone = out_addr.clone();

    // The closure inside `make_service_fn` is run for each connection,
    // creating a 'service' to handle requests for that specific connection.
    let make_service = make_service_fn(move |_| {
        let client = client_main.clone();

        async move {
            // This is the `Service` that will handle the connection.
            // `service_fn` is a helper to convert a function that
            // returns a Response into a `Service`.
            Ok::<_, GenericError>(service_fn(move |req| {
                // Clone again to ensure that client outlives this closure.
                response_examples(req, client.to_owned())
            }))
        }
    });

    let server = Server::bind(&in_addr).serve(make_service);

    println!("Listening on http://{}", in_addr);
    println!("Proxying on http://{}", out_addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
