use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts, State},
    http::request::Parts,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use axum_extra::extract::{
    cookie::{self, Cookie},
    PrivateCookieJar,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{AppState, InternalError};

static COOKIE_NAME: &str = "altreg_session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    username: String,
    /// Argon2id hashed password
    password: String,
    blocked: bool,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(auth_me))
        .route("/auth/logout", get(auth_logout))
}

async fn auth_me(AuthSession(username, jar): AuthSession) -> (PrivateCookieJar, String) {
    (jar, format!("hello {username}"))
}


async fn auth_logout(
    State(_db): State<crate::Db>,
    session: Result<AuthSession, UnauthSession>,
) -> (PrivateCookieJar, Redirect) {
    let jar = match session {
        Ok(AuthSession(username, jar)) => {
            debug!("user {username} logged out");
            // TODO: Remove session from the database
            jar.remove(Cookie::named(COOKIE_NAME))
        }
        Err(UnauthSession(jar)) => jar,
    };

    (jar, Redirect::temporary("/"))
}

struct AuthSession(String, PrivateCookieJar);
struct UnauthSession(PrivateCookieJar);

#[async_trait]
impl<S> FromRequestParts<S> for AuthSession
where
    S: Send + Sync,
    cookie::Key: FromRef<S>,
{
    type Rejection = UnauthSession;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jar = PrivateCookieJar::from_request_parts(parts, state)
            .await
            .expect("infallible result");

        // Unauthorized if they don't have a correctly signed cookie
        let Some(cookie) = jar.get(COOKIE_NAME) else {
            dbg!(&jar);
            let jar = jar.add(Cookie::new(COOKIE_NAME, "hello"));
            dbg!(&jar);
            return Err(UnauthSession(jar));
        };

        let username = cookie.value();
        let jar = jar.remove(Cookie::named(COOKIE_NAME));

        // TODO: check auth is valid

        return Ok(AuthSession(username.to_owned(), jar));
    }
}

impl IntoResponse for UnauthSession {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, self.0, "unauthorized").into_response()
    }
}
