use std::path;

use axum::{
    extract::Path,
    response::{Html, Redirect},
    routing::get,
    Extension, Router,
};
use chrono_humanize::HumanTime;
use reqwest::StatusCode;
use tera::Tera;
use tower_http::services::ServeDir;

use crate::{Entry, InternalError};

pub fn router(data_dir: &path::Path) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/crates", get(crate_list))
        .route("/crates/:crate_name", get(crate_root))
        .route("/crates/:crate_name/:version", get(crate_view))
        .nest(
            "/docs",
            axum::routing::get_service(ServeDir::new(data_dir.join("docs"))).handle_error(
                |error: std::io::Error| async move {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Unhandled internal error: {}", error),
                    )
                },
            ),
        )
}

async fn crate_list(
    Extension(db): Extension<sled::Db>,
    Extension(tera): Extension<Tera>,
) -> Result<Html<String>, InternalError> {
    let crates: Vec<String> = db
        .iter()
        .filter_map(|elem| elem.ok())
        .map(|(crate_name, _)| String::from_utf8_lossy(&crate_name).to_string())
        .collect();

    let mut context = tera::Context::new();
    context.insert("crates", &crates);
    let body = tera.render("crates.html", &context)?;
    Ok(Html(body))
}

async fn root() -> Redirect {
    Redirect::permanent("/crates")
}

async fn crate_root(Path(crate_name): Path<String>) -> Redirect {
    Redirect::temporary(&format!("/crates/{}/latest", crate_name))
}

async fn crate_view(
    Path((crate_name, mut version)): Path<(String, String)>,
    Extension(db): Extension<sled::Db>,
    Extension(tera): Extension<Tera>,
) -> Result<Html<String>, InternalError> {
    let crate_meta = match db.get(&crate_name)? {
        Some(entry) => bincode::deserialize::<Entry>(&entry)?,
        None => {
            let body = tera.render("crate_not_found.html", &tera::Context::new())?;
            return Ok(Html(body));
        }
    };

    let is_local = crate_meta.is_local;
    let versions = crate_meta
        .versions
        .iter()
        .map(|package| package.pkg.vers.clone())
        .collect::<Vec<_>>();

    let meta = if version == "latest" {
        let meta = crate_meta.versions.last().unwrap();
        version = meta.pkg.vers.clone();
        meta
    } else {
        match crate_meta
            .versions
            .iter()
            .find(|package| package.pkg.vers == version)
        {
            Some(package) => package,
            None => {
                let body = tera.render("crate_not_found.html", &tera::Context::new())?;
                return Ok(Html(body));
            }
        }
    };

    let readme = meta
        .upload_meta
        .as_ref()
        .and_then(|meta| meta.readme.as_ref())
        .map(|readme| comrak::markdown_to_html(readme, &comrak::ComrakOptions::default()));

    let time_since_upload = meta
        .upload_timestamp
        .map(|ts| HumanTime::from(ts).to_string());

    let mut context = tera::Context::new();
    context.insert("crate_name", &crate_name);
    context.insert("time_since_upload", &time_since_upload);
    context.insert("version", &version);
    context.insert("meta", &meta);
    context.insert("is_local", &is_local);
    context.insert("rendered_readme", &readme);
    context.insert("versions", &versions);
    let body = tera.render("crate.html", &context)?;
    Ok(Html(body))
}
