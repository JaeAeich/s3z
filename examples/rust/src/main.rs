//! Axum server demonstrating s3z — S3 ops, but fearlessly fast.

mod error;
mod models;
mod routes;
mod state;

use axum::{Router, routing::get};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

use crate::state::AppState;

/// s3z API documentation.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "s3z",
        version = "0.1.0",
        description = "A thin REST layer over s3z. All heavy lifting (multipart upload/download, \
                        connection pooling, SigV4 signing) happens in the native Rust library."
    ),
    paths(routes::upload, routes::download, routes::list),
    components(schemas(models::UploadFileResult, models::ObjectInfo)),
    tags(
        (name = "s3", description = "S3 operations powered by s3z.")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = AppState::from_env().await?;

    let app = Router::new()
        .route("/upload", axum::routing::post(routes::upload))
        .route("/download", get(routes::download))
        .route("/list", get(routes::list))
        .merge(Scalar::with_url("/docs", ApiDoc::openapi()))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    println!("s3z axum server running at http://localhost:8080");
    println!("API docs at http://localhost:8080/docs");
    axum::serve(listener, app).await?;
    Ok(())
}
