mod api;
mod auth;
mod config;
mod db;
mod dl;
mod docs;
mod index;
mod mirror;
mod package;
mod token;
mod ui;

use axum_extra::extract::cookie;
use axum_server::tls_rustls::RustlsConfig;
use db::Db;

use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Context;
use axum::{extract::FromRef, http::StatusCode, response::IntoResponse, Router};
use config::Config;
use package::UploadedPackage;
use serde::{Deserialize, Serialize};
use tera::Tera;
use tokio::sync::mpsc::{self, UnboundedSender};
use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Serialize, Deserialize)]
pub struct Entry {
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

#[derive(Clone, FromRef)]
pub struct AppState {
    cookie_key: cookie::Key,
    config: Config,
    db: db::Db,
    templates: Tera,
    docs_queue_tx: UnboundedSender<(String, String)>,
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

    let db = db::Db::open(config.data_dir.join("db"))?;

    // Docs generator thread
    let (docs_queue_tx, docs_queue_rx) = mpsc::unbounded_channel();
    docs::start_background_thread(config.data_dir.clone(), docs_queue_rx);

    let tera =
        Tera::new("templates/**.html").with_context(|| "unable to load templates".to_owned())?;
    let listen_addr = SocketAddr::new(config.host, config.port);

    let app = Router::new()
        .merge(ui::router(&config.data_dir))
        .merge(dl::router())
        .merge(auth::router())
        .nest("/index", index::router())
        .nest("/api", api::router())
        .nest_service(
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
        .with_state(AppState {
            config,
            db,
            templates: tera,
            docs_queue_tx,
            cookie_key: cookie::Key::generate(),
        })
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    let config = RustlsConfig::from_pem_file("localhost.pem", "localhost-key.pem")
        .await
        .unwrap();

    axum_server::bind_rustls(listen_addr, config)
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
