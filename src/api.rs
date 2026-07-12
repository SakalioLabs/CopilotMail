use crate::{
    config::Config,
    events::Events,
    outbound, smtp,
    store::{Mailbox, Message, MessageSummary, Store},
};
use anyhow::Result;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
};
use uuid::Uuid;

#[derive(Clone)]
struct App {
    config: Config,
    store: Store,
    events: Events,
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
#[derive(Clone, Serialize, Deserialize)]
struct Claims {
    sub: String,
    address: String,
    exp: usize,
}
#[derive(Deserialize)]
struct SeenInput {
    seen: bool,
}
#[derive(Deserialize)]
struct SendInput {
    to: String,
    subject: String,
    text: String,
    html: Option<String>,
}
#[derive(Serialize)]
struct SendResult {
    id: String,
    status: &'static str,
}
#[derive(Deserialize)]
struct WsAuth {
    token: String,
}

type ApiResult<T> = Result<T, (StatusCode, String)>;

pub async fn serve(config: Config, store: Store, events: Events) -> Result<()> {
    let addr: SocketAddr = config.http_addr;
    let web_dir = config.web_dir.clone();
    let state = Arc::new(App {
        config,
        store,
        events,
    });
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/mailboxes", post(create_mailbox))
        .route(
            "/v1/mailboxes/{id}",
            get(mailbox).delete(delete_current_mailbox),
        )
        .route("/v1/auth/token", post(login))
        .route("/v1/messages", get(messages))
        .route("/v1/messages/send", post(send_message))
        .route(
            "/v1/messages/{id}",
            get(message).patch(set_seen).delete(delete_message),
        )
        .route("/v1/events", get(ws_events))
        .fallback_service(
            ServeDir::new(&web_dir).fallback(ServeFile::new(web_dir.join("index.html"))),
        )
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

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok","service":"copilotmail"}))
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
        exp: (Utc::now().timestamp() + 86400) as usize,
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

async fn set_seen(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<SeenInput>,
) -> ApiResult<StatusCode> {
    let claims = user(&app, &headers)?;
    if app
        .store
        .set_seen(&claims.sub, &id, input.seen)
        .map_err(internal)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "message not found".into()))
    }
}

async fn delete_message(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let claims = user(&app, &headers)?;
    if app
        .store
        .delete_message(&claims.sub, &id)
        .map_err(internal)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "message not found".into()))
    }
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

async fn delete_current_mailbox(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let claims = user(&app, &headers)?;
    if claims.sub != id {
        return Err((StatusCode::FORBIDDEN, "forbidden".into()));
    }
    app.store.delete_mailbox(&id).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn send_message(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<SendInput>,
) -> ApiResult<(StatusCode, Json<SendResult>)> {
    let claims = user(&app, &headers)?;
    validate_header(&input.to)?;
    validate_header(&input.subject)?;
    let id = Uuid::new_v4().to_string();
    let raw = build_message(&id, &claims.address, &input);
    if input
        .to
        .to_ascii_lowercase()
        .ends_with(&format!("@{}", app.config.domain))
    {
        if app
            .store
            .mailbox_by_address(&input.to.to_ascii_lowercase())
            .map_err(internal)?
            .is_none()
        {
            return Err((StatusCode::NOT_FOUND, "local mailbox not found".into()));
        }
        smtp::persist(
            &app.store,
            &app.events,
            &claims.address,
            &[input.to.to_ascii_lowercase()],
            &raw,
        )
        .map_err(internal)?;
    } else {
        outbound::deliver(&claims.address, &input.to, &raw)
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, format!("delivery failed: {error}")))?;
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(SendResult {
            id,
            status: "accepted",
        }),
    ))
}

async fn ws_events(
    State(app): State<Arc<App>>,
    Query(auth): Query<WsAuth>,
    upgrade: WebSocketUpgrade,
) -> ApiResult<impl IntoResponse> {
    let claims = decode_token(&app, &auth.token)?;
    Ok(upgrade.on_upgrade(move |socket| event_socket(socket, app.events.clone(), claims.sub)))
}

async fn event_socket(mut socket: WebSocket, events: Events, mailbox_id: String) {
    let mut receiver = events.subscribe();
    while let Ok(event) = receiver.recv().await {
        if event.mailbox_id == mailbox_id {
            let Ok(payload) = serde_json::to_string(&event) else {
                continue;
            };
            if socket.send(WsMessage::Text(payload.into())).await.is_err() {
                break;
            }
        }
    }
}

fn build_message(id: &str, sender: &str, input: &SendInput) -> Vec<u8> {
    let boundary = format!("copilotmail-{id}");
    let html = input.html.as_deref().unwrap_or("");
    format!("From: <{sender}>\r\nTo: <{}>\r\nSubject: {}\r\nMessage-ID: <{id}@copilotmail>\r\nDate: {}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n--{boundary}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{html}\r\n--{boundary}--\r\n", input.to, input.subject, Utc::now().to_rfc2822(), input.text).into_bytes()
}

fn validate_header(value: &str) -> ApiResult<()> {
    if value.contains(['\r', '\n']) {
        Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "header contains a newline".into(),
        ))
    } else {
        Ok(())
    }
}
fn normalize(address: &str, domain: &str) -> ApiResult<String> {
    let address = address.trim().to_ascii_lowercase();
    let Some((local, actual_domain)) = address.split_once('@') else {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid address".into()));
    };
    if local.len() < 3 || actual_domain != domain || address.matches('@').count() != 1 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("address must be at {domain}"),
        ));
    }
    Ok(address)
}
fn admin(app: &App, headers: &HeaderMap) -> ApiResult<()> {
    if headers.get("x-admin-key").and_then(|v| v.to_str().ok()) == Some(&app.config.admin_key) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "invalid admin key".into()))
    }
}
fn user(app: &App, headers: &HeaderMap) -> ApiResult<Claims> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".into()))?;
    decode_token(app, token)
}
fn decode_token(app: &App, token: &str) -> ApiResult<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(app.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|v| v.claims)
    .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid bearer token".into()))
}
fn internal<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    tracing::error!(%error,"request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".into(),
    )
}
fn conflict<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    if error.to_string().contains("UNIQUE constraint") {
        (StatusCode::CONFLICT, "mailbox already exists".into())
    } else {
        internal(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_header_injection() {
        assert!(validate_header("hello\r\nBcc: victim@example.test").is_err());
    }
    #[test]
    fn creates_multipart_message() {
        let raw = build_message(
            "id",
            "agent@mail.local",
            &SendInput {
                to: "dest@example.test".into(),
                subject: "test".into(),
                text: "plain".into(),
                html: Some("<b>html</b>".into()),
            },
        );
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("multipart/alternative"));
        assert!(text.contains("<b>html</b>"));
    }
}
