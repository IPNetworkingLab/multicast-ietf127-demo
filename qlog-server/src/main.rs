// CODE FROM https://github.com/tokio-rs/axum/blob/main/examples/stream-to-file/src/main.rs

use axum::{
    BoxError, Router,
    body::Bytes,
    extract::{Path, Request},
    http::StatusCode,
    response::Html,
    routing::{get, put},
};
use futures_util::{Stream, TryStreamExt};
use log::{debug, info};
use std::{io, pin::pin};
use tokio::{fs::File, io::BufWriter};
use tokio_util::io::StreamReader;

const UPLOADS_DIRECTORY: &str = "uploaded-qlogs";

#[tokio::main]
async fn main() {
    env_logger::builder().format_timestamp_nanos().init();

    // save files to a separate directory to not override files in the current directory
    match tokio::fs::create_dir(UPLOADS_DIRECTORY).await {
        Ok(_) => (),
        Err(e) => info!("Failed to create uploaded-qlogs dir: {e:?}"),
    }

    let app = Router::new()
        .route("/", get(show_homepage))
        .route("/qlog/{file_name}", put(save_request_body));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap();
    debug!("listening on http://{}", listener.local_addr().unwrap());
    println!("listening on http://{}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

// Handler that streams the request body to a file.
//
// POST'ing to `/qlog/foo.txt` will create a file called `foo.txt`.
async fn save_request_body(
    Path(file_name): Path<String>,
    request: Request,
) -> Result<(), (StatusCode, String)> {
    info!("Saving received file={file_name}");
    stream_to_file(&file_name, request.into_body().into_data_stream()).await
}

async fn show_homepage() -> Html<&'static str> {
    Html(
        r#"
        <!doctype html>
        <html>
            <head>
                <title>FCQUIC Source</title>
            </head>
            <body>
                <h1>Flexicast QUIC source demo</h1>
                <p>This server is running two FCQUIC sources, see <a href="https://github.com/IPNetworkingLab/multicast-ietf127-demo">https://github.com/IPNetworkingLab/multicast-ietf127-demo</a>for more information</p>
            </body>
        </html>
        "#,
    )
}

// Save a `Stream` to a file
async fn stream_to_file<S, E>(path: &str, stream: S) -> Result<(), (StatusCode, String)>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Into<BoxError>,
{
    if !path_is_valid(path) {
        return Err((StatusCode::BAD_REQUEST, "Invalid path".to_owned()));
    }

    async {
        // Convert the stream into an `AsyncRead`.
        let body_with_io_error = stream.map_err(io::Error::other);
        let mut body_reader = pin!(StreamReader::new(body_with_io_error));

        // Create the file. `File` implements `AsyncWrite`.
        let path = std::path::Path::new(UPLOADS_DIRECTORY).join(path);
        let mut file = BufWriter::new(File::create(path).await?);

        // Copy the body into the file.
        tokio::io::copy(&mut body_reader, &mut file).await?;

        Ok::<_, io::Error>(())
    }
    .await
    .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

// to prevent directory traversal attacks we ensure the path consists of exactly one normal
// component
fn path_is_valid(path: &str) -> bool {
    let path = std::path::Path::new(path);
    let mut components = path.components().peekable();

    if let Some(first) = components.peek() {
        if !matches!(first, std::path::Component::Normal(_)) {
            return false;
        }
    }

    components.count() == 1
}
