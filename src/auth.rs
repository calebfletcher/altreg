use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts, State},
    http::request::Parts,
    response::{Html, IntoResponse, Redirect, Response},
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
use tracing::{debug, info};

use crate::{token, AppState, InternalError};

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
        .route(
            "/auth/tokens",
            get(auth_tokens_page).post(auth_token_create),
        )
        .route("/auth/tokens/delete", post(auth_tokens_delete))
        .route("/auth/login", get(auth_login_page).post(auth_login))
        .route("/auth/logout", get(auth_logout))
        .route(
            "/auth/register",
            get(auth_register_page).post(auth_register),
        )
}

async fn auth_me(AuthSession(username, jar): AuthSession) -> (PrivateCookieJar, String) {
    (jar, format!("hello {username}"))
}

#[derive(Deserialize)]
struct LoginParams {
    username: String,
    password: String,
}

async fn auth_login(
    State(db): State<crate::Db>,
    State(tera): State<tera::Tera>,
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
        return auth_login_page(State(tera), Some("non-existent user".into())).await.map(|resp| resp.into_response());
    };

    // Check user password
    let parsed_hash = PasswordHash::new(&user.password)?;
    if Argon2::default()
        .verify_password(login.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        // Incorrect password
        return auth_login_page(State(tera), Some("incorrect password".into()))
            .await
            .map(|resp| resp.into_response());
    }

    if user.blocked {
        return auth_login_page(State(tera), Some("user blocked".into()))
            .await
            .map(|resp| resp.into_response());
    }

    info!("user {} logged in", login.username);

    // Set cookies
    let jar = set_auth_cookie(jar, login.username);

    Ok((StatusCode::OK, jar, "auth success").into_response())
}

async fn auth_login_page(
    State(tera): State<tera::Tera>,
    warning: Option<String>,
) -> Result<Html<String>, InternalError> {
    let mut context = tera::Context::new();
    if let Some(warning) = warning {
        context.insert("warning", &warning);
    }
    let body = tera.render("login.html", &context)?;
    Ok(Html(body))
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

#[derive(Deserialize)]
struct RegisterParams {
    username: String,
    password: String,
}
async fn auth_register(
    State(db): State<crate::Db>,
    State(tera): State<tera::Tera>,
    session: Result<AuthSession, UnauthSession>,
    Form(login): Form<RegisterParams>,
) -> Result<Response, InternalError> {
    let jar = match session {
        Ok(AuthSession(_username, jar)) => {
            // User already has a cookie, check if it has expired
            return Ok((StatusCode::OK, jar, Redirect::temporary("/")).into_response());
        }
        Err(UnauthSession(jar)) => jar,
    };

    if db.get_user(&login.username)?.is_some() {
        // User already exists in database
        return auth_register_page(State(tera), Some("user already exists".into()))
            .await
            .map(|resp| resp.into_response());
    };

    // Hash password
    let salt = argon2::password_hash::SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(login.password.as_bytes(), &salt)?
        .to_string();

    // Create user in database
    let user = User {
        username: login.username.clone(),
        password: password_hash,
        blocked: false,
    };
    db.insert_user(&user.username, &user)?;

    info!("user {} registered", login.username);

    // Set cookie
    let jar = set_auth_cookie(jar, login.username);

    Ok((StatusCode::OK, jar, "register success").into_response())
}

async fn auth_register_page(
    State(tera): State<tera::Tera>,
    warning: Option<String>,
) -> Result<Html<String>, InternalError> {
    let mut context = tera::Context::new();
    if let Some(warning) = warning {
        context.insert("warning", &warning);
    }
    let body = tera.render("register.html", &context)?;
    Ok(Html(body))
}

#[derive(Deserialize)]
struct TokenParams {
    label: String,
}
async fn auth_token_create(
    AuthSession(username, jar): AuthSession,
    State(db): State<crate::Db>,
    State(tera): State<tera::Tera>,
    Form(params): Form<TokenParams>,
) -> Result<impl IntoResponse, InternalError> {
    let token = token::create_token(&db, &username, &params.label)?;

    auth_tokens_page(AuthSession(username, jar), State(db), State(tera), token).await
}

async fn auth_tokens_page(
    AuthSession(username, jar): AuthSession,
    State(db): State<crate::Db>,
    State(tera): State<tera::Tera>,
    token: Option<String>,
) -> Result<impl IntoResponse, InternalError> {
    let mut context = tera::Context::new();
    if let Some(token) = token {
        context.insert("token", &token);
    }

    context.insert("token_entries", &token::get_user_tokens(&db, &username)?);

    let body = tera.render("tokens.html", &context)?;
    Ok((jar, Html(body)))
}

async fn auth_tokens_delete(
    AuthSession(username, jar): AuthSession,
    State(db): State<crate::Db>,
    Form(params): Form<TokenParams>,
) -> Result<impl IntoResponse, InternalError> {
    token::delete(&db, &username, &params.label)?;

    Ok((jar, Redirect::to("/auth/tokens")))
}

fn set_auth_cookie(jar: PrivateCookieJar, username: String) -> PrivateCookieJar {
    jar.add(
        Cookie::build(COOKIE_NAME, username)
            .path("/")
            .http_only(true)
            .finish(),
    )
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

        // TODO: check auth is valid

        return Ok(AuthSession(username.to_owned(), jar));
    }
}

impl IntoResponse for UnauthSession {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, self.0, "unauthorized").into_response()
    }
}
