use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};

use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::{AppState, auth};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

pub struct SessionUser {
    pub user_id: String,
    pub is_admin: bool,
}

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token =
            auth::session_token_from_cookie(&parts.headers).ok_or(StatusCode::UNAUTHORIZED)?;
        let hash = auth::token_hash(&token);
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT user_id, expires_at FROM sessions WHERE token_hash = ?")
                .bind(&hash)
                .fetch_optional(&state.pool)
                .await
                .unwrap();
        match row {
            Some((uid, exp)) if exp > now() => {
                let is_admin: i64 = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?")
                    .bind(&uid)
                    .fetch_one(&state.pool)
                    .await
                    .unwrap_or(0);
                Ok(SessionUser {
                    user_id: uid,
                    is_admin: is_admin != 0,
                })
            }
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }
}

fn client_ip(headers: &HeaderMap) -> IpAddr {
    // ULTIMO elemento di X-Forwarded-For: quello lo aggiunge il proxy fidato
    // (tailscale serve); il primo è controllabile dal client e falsifica la chiave.
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

#[derive(Deserialize)]
pub struct Creds {
    pub email: String,
    pub password: String,
}

pub async fn register(State(s): State<AppState>, Json(c): Json<Creds>) -> Response {
    if c.password.len() < 8 {
        return err(StatusCode::BAD_REQUEST, "password minima 8 caratteri");
    }
    let phc = auth::hash(c.password.clone()).await;
    let id = uuid::Uuid::new_v4().to_string();
    let r = sqlx::query(
        "INSERT INTO users (id, email, password_hash, is_admin, created_at)
         SELECT ?, ?, ?, 1, ? WHERE (SELECT COUNT(*) FROM users) = 0",
    )
    .bind(&id)
    .bind(&c.email)
    .bind(&phc)
    .bind(now())
    .execute(&s.pool)
    .await
    .unwrap();
    if r.rows_affected() == 0 {
        return err(StatusCode::FORBIDDEN, "registrazione chiusa");
    }
    Json(serde_json::json!({"id": id})).into_response()
}

pub async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(c): Json<Creds>,
) -> Response {
    let ip_key = client_ip(&headers).to_string();
    let mail_key = format!("mail:{}", c.email.to_lowercase());
    s.limiter.reap();
    if let Err(wait) = s.limiter.check(&ip_key).and(s.limiter.check(&mail_key)) {
        return err(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("riprova tra {wait} secondi"),
        );
    }
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
            .bind(&c.email)
            .fetch_optional(&s.pool)
            .await
            .unwrap();
    let Some((uid, phc)) = row else {
        s.limiter.fail(&ip_key);
        s.limiter.fail(&mail_key);
        return err(StatusCode::UNAUTHORIZED, "credenziali non valide");
    };
    if !auth::verify(phc, c.password.clone()).await {
        s.limiter.fail(&ip_key);
        s.limiter.fail(&mail_key);
        return err(StatusCode::UNAUTHORIZED, "credenziali non valide");
    }
    s.limiter.success(&ip_key);
    s.limiter.success(&mail_key);
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(now())
        .execute(&s.pool)
        .await
        .unwrap();
    let token = auth::new_token();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?,?,?)")
        .bind(auth::token_hash(&token))
        .bind(&uid)
        .bind(now() + auth::COOKIE_MAX_AGE)
        .execute(&s.pool)
        .await
        .unwrap();
    (
        [(axum::http::header::SET_COOKIE, auth::cookie_for(&token))],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}

pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = auth::session_token_from_cookie(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(auth::token_hash(&t))
            .execute(&s.pool)
            .await
            .unwrap();
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn me(user: SessionUser, State(s): State<AppState>) -> Response {
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_one(&s.pool)
        .await
        .unwrap();
    Json(serde_json::json!({"email": email, "is_admin": user.is_admin})).into_response()
}

pub async fn admin_create(
    user: SessionUser,
    State(s): State<AppState>,
    Json(c): Json<Creds>,
) -> Response {
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "solo admin");
    }
    if c.password.len() < 8 {
        return err(StatusCode::BAD_REQUEST, "password minima 8 caratteri");
    }
    let phc = auth::hash(c.password.clone()).await;
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO users (id,email,password_hash,is_admin,created_at) VALUES (?,?,?,0,?)",
    )
    .bind(&id)
    .bind(&c.email)
    .bind(&phc)
    .bind(now())
    .execute(&s.pool)
    .await
    {
        if e.as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false)
        {
            return err(StatusCode::CONFLICT, "email già in uso");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    Json(serde_json::json!({"id": id})).into_response()
}

/* reset-password CLI in main.rs (emergenza locale sul gateway):
args SOLO reset-password <email> (la nuova password LETTA DA STDIN, mai argv —
resterebbe nella history della shell). spawn_blocking hash + UPDATE users +
DELETE FROM sessions WHERE user_id = ?; poi exit. VAULTI_DATA_DIR coerente. */
