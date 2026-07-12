use crate::{
    config::Config,
    store::{Mailbox, Message, MessageSummary, Store},
};
use anyhow::Result;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
struct App {
    config: Config,
    store: Store,
}
#[derive(Deserialize)]
struct Credentials {
    address: String,
    password: String,
}
#[derive(Serialize)]
struct Token {
    token: String,
    mailbox: Mailbox,
}
#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    address: String,
    exp: usize,
}
type ApiResult<T> = Result<T, (StatusCode, String)>;

pub async fn serve(config: Config, store: Store) -> Result<()> {
    let addr: SocketAddr = config.http_addr;
    let state = Arc::new(App { config, store });
    let app = Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"status":"ok","service":"copilotmail"})) }),
        )
        .route("/v1/mailboxes", post(create_mailbox))
        .route("/v1/auth/token", post(login))
        .route("/v1/messages", get(messages))
        .route("/v1/messages/{id}", get(message))
        .route("/v1/mailboxes/{id}", get(mailbox))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn create_mailbox(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Credentials>,
) -> ApiResult<(StatusCode, Json<Mailbox>)> {
    admin(&app, &headers)?;
    let address = normalize(&input.address, &app.config.domain)?;
    if input.password.len() < 8 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "password must be at least 8 characters".into(),
        ));
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(input.password.as_bytes(), &salt)
        .map_err(internal)?
        .to_string();
    let mailbox = app
        .store
        .create_mailbox(&address, &hash)
        .map_err(conflict)?;
    Ok((StatusCode::CREATED, Json(mailbox)))
}
async fn login(
    State(app): State<Arc<App>>,
    Json(input): Json<Credentials>,
) -> ApiResult<Json<Token>> {
    let address = normalize(&input.address, &app.config.domain)?;
    let (mailbox, hash) = app
        .store
        .mailbox_auth(&address)
        .map_err(internal)?
        .ok_or((StatusCode::UNAUTHORIZED, "invalid credentials".into()))?;
    let parsed = PasswordHash::new(&hash).map_err(internal)?;
    Argon2::default()
        .verify_password(input.password.as_bytes(), &parsed)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid credentials".into()))?;
    let claims = Claims {
        sub: mailbox.id.clone(),
        address: mailbox.address.clone(),
        exp: (chrono::Utc::now().timestamp() + 86400) as usize,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(app.config.jwt_secret.as_bytes()),
    )
    .map_err(internal)?;
    Ok(Json(Token { token, mailbox }))
}
async fn messages(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<MessageSummary>>> {
    let claims = user(&app, &headers)?;
    Ok(Json(app.store.messages(&claims.sub).map_err(internal)?))
}
async fn message(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Message>> {
    let claims = user(&app, &headers)?;
    let item = app
        .store
        .message(&claims.sub, &id)
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "message not found".into()))?;
    Ok(Json(item))
}
async fn mailbox(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Mailbox>> {
    let claims = user(&app, &headers)?;
    if claims.sub != id {
        return Err((StatusCode::FORBIDDEN, "forbidden".into()));
    }
    let (item, _) = app
        .store
        .mailbox_auth(&claims.address)
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "not found".into()))?;
    Ok(Json(item))
}
fn normalize(address: &str, domain: &str) -> ApiResult<String> {
    let a = address.trim().to_ascii_lowercase();
    let mut p = a.split('@');
    let local = p.next().unwrap_or("");
    let d = p.next().unwrap_or("");
    if local.len() < 3 || d != domain || p.next().is_some() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("address must be at {domain}"),
        ));
    }
    Ok(a)
}
fn admin(app: &App, h: &HeaderMap) -> ApiResult<()> {
    if h.get("x-admin-key").and_then(|v| v.to_str().ok()) == Some(&app.config.admin_key) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "invalid admin key".into()))
    }
}
fn user(app: &App, h: &HeaderMap) -> ApiResult<Claims> {
    let token = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".into()))?;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(app.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|v| v.claims)
    .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid bearer token".into()))
}
fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    tracing::error!(error=%e,"request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".into(),
    )
}
fn conflict<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    if e.to_string().contains("UNIQUE constraint") {
        (StatusCode::CONFLICT, "mailbox already exists".into())
    } else {
        internal(e)
    }
}
