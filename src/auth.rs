use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
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
use rand::rngs::OsRng;
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
        .route("/auth", post(auth_login_page))
        .route("/auth/logout", get(auth_logout))
}

async fn auth_me(AuthSession(username, jar): AuthSession) -> (PrivateCookieJar, String) {
    (jar, format!("hello {username}"))
}

#[derive(Deserialize)]
struct LoginParams {
    username: String,
    password: String,
}

async fn auth_login_page(
    State(db): State<crate::Db>,
    session: Result<AuthSession, UnauthSession>,
    Form(login): Form<LoginParams>,
) -> Result<Response, InternalError> {
    let jar = match session {
        Ok(AuthSession(_username, jar)) => {
            // User already has a cookie, check if it has expired
            return Ok((StatusCode::OK, jar, Redirect::temporary("/")).into_response());
        }
        Err(UnauthSession(jar)) => jar,
    };

    let Some(user) = db.get_user(&login.username)? else {
        // User doesn't exist in database

        // TODO: don't create a user lmao
        let salt = argon2::password_hash::SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2.hash_password(login.password.as_bytes(), &salt)?.to_string();
        let user = User { username: login.username, password: password_hash, blocked: false };
        db.insert_user(&user.username, &user)?;

        return Ok((StatusCode::UNAUTHORIZED, jar, "non-existent user").into_response())
    };

    // Check user password
    let parsed_hash = PasswordHash::new(&user.password)?;
    if Argon2::default()
        .verify_password(login.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        // Incorrect password
        return Ok((StatusCode::UNAUTHORIZED, jar, "incorrect password").into_response());
    }

    if user.blocked {
        return Ok((StatusCode::FORBIDDEN, jar, "user blocked").into_response());
    }

    // Set cookies
    let jar = jar.add(Cookie::new(COOKIE_NAME, login.username));

    Ok((StatusCode::OK, jar, "auth success").into_response())
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
