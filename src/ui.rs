use axum::{
    extract::Path,
    response::{Html, Redirect},
    routing::get,
    Extension, Router,
};
use tera::Tera;

use crate::{Entry, InternalError};

pub fn router() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/crates", get(crate_list))
        .route("/crates/:crate_name", get(crate_view))
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

async fn crate_view(
    Path(crate_name): Path<String>,
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

    let mut context = tera::Context::new();
    context.insert("crate_name", &crate_name);
    context.insert("meta", &crate_meta);
    let body = tera.render("crate.html", &context)?;
    Ok(Html(body))
}
