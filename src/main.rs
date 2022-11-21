mod api;
mod config;
mod dl;
mod docs;
mod index;
mod mirror;
mod package;
mod ui;

use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Context;
use axum::{http::StatusCode, response::IntoResponse, Extension, Router};
use package::UploadedPackage;
use serde::{Deserialize, Serialize};
use tera::Tera;
use tokio::sync::mpsc;
use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Serialize, Deserialize)]
struct Entry {
    versions: Vec<UploadedPackage>,
    time_of_last_update: chrono::DateTime<chrono::Utc>,
    is_local: bool,
}

struct InternalError(anyhow::Error);

impl IntoResponse for InternalError {
    fn into_response(self) -> axum::response::Response {
        tracing::warn!("stacktrace: {:?}", self.0);
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "something went wrong",
        )
            .into_response()
    }
}

impl<T: Into<anyhow::Error>> From<T> for InternalError {
    fn from(e: T) -> Self {
        Self(e.into())
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialise logging system
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "altreg=info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::load().with_context(|| "unable to load config")?;

    // Directory checks
    if !config.data_dir.exists() {
        fs::create_dir(&config.data_dir).with_context(|| "unable to create data dir")?;
    }
    let crates_dir = config.data_dir.join("crates");
    if !crates_dir.exists() {
        fs::create_dir(&crates_dir).with_context(|| "unable to create crate cache dir")?;
    }

    let db = sled::open(config.data_dir.join("db"))
        .with_context(|| format!("unable to open database in {}", config.data_dir.display()))?;

    // Docs generator thread
    let (docs_queue_tx, docs_queue_rx) = mpsc::unbounded_channel();
    docs::start_background_thread(config.data_dir.clone(), docs_queue_rx);

    let tera =
        Tera::new("templates/**.html").with_context(|| "unable to load templates".to_owned())?;
    let listen_addr = SocketAddr::new(config.host, config.port);

    let app = Router::new()
        .merge(ui::router(&config.data_dir))
        .merge(dl::router())
        .nest("/index", index::router())
        .nest("/api", api::router())
        .nest(
            "/static",
            axum::routing::get_service(ServeDir::new("static")).handle_error(
                |error: std::io::Error| async move {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Unhandled internal error: {}", error),
                    )
                },
            ),
        )
        .layer(Extension(config))
        .layer(Extension(db))
        .layer(Extension(tera))
        .layer(Extension(docs_queue_tx))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    axum::Server::bind(&listen_addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

fn crate_path(data_dir: impl AsRef<Path>, name: &str, version: &str) -> PathBuf {
    data_dir
        .as_ref()
        .join("crates")
        .join(name)
        .join(version.to_owned() + ".crate")
}
