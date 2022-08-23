mod config;
pub mod mirror;
pub mod package;

use axum::{
    body::Bytes,
    extract::Path,
    http::Uri,
    routing::{get, put},
    Json, Router,
};
use config::Config;
use serde_json::{json, Value};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

async fn index() -> Json<Value> {
    Json(json!({ "dl": "http://localhost:3000/crates", "api": "http://localhost:3000" }))
}

async fn crate_fallback(uri: Uri) -> String {
    let parts = uri
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let crate_name = parts.last().unwrap();

    // let package = Package {
    //     name: crate_name.to_string(),
    //     vers: "4.2.0".to_owned(),
    //     deps: vec![],
    //     cksum: "".to_owned(),
    //     features: HashMap::new(),
    //     yanked: false,
    //     links: None,
    //     v: 2,
    //     features2: HashMap::new(),
    // };

    //serde_json::to_string(&package).unwrap()

    mirror::get_package(crate_name).await.unwrap()
}

async fn crate_download(Path((crate_name, version)): Path<(String, String)>) -> Bytes {
    println!("downloading {} {}", crate_name, version);

    mirror::download_crate(&crate_name, version).await.unwrap()
}

async fn add_crate(body: Bytes) {
    dbg!(body);
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialise logging system
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "altreg=debug,tower_http=info".into()),
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

    let listen_addr = SocketAddr::new(config.host, config.port);

    let app = Router::new()
        .route("/config.json", get(index))
        .route("/crates/:crate_name/:version/download", get(crate_download))
        .route("/api/v1/crates/new", put(add_crate))
        .fallback(get(crate_fallback))
        .layer(Extension(config))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    axum::Server::bind(&listen_addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

pub fn crate_prefix(name: &str) -> String {
    match name.len() {
        1 => "1".to_owned(),
        2 => "2".to_owned(),
        3 => format!("3/{}", name.chars().next().unwrap()),
        _ => {
            let chars: Vec<_> = name.chars().take(4).collect();
            format!("{}{}/{}{}", chars[0], chars[1], chars[2], chars[3])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_prefix_samples() {
        assert_eq!(crate_prefix("a"), "1");
        assert_eq!(crate_prefix("ab"), "2");
        assert_eq!(crate_prefix("abc"), "3/a");
        assert_eq!(crate_prefix("cargo"), "ca/rg");
    }
}
