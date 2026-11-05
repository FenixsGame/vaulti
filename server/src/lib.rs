pub mod auth;
pub mod config;
pub mod db;
pub mod handlers_auth;

use axum::Router;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::routing::post;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<String>,
    pub limiter: Arc<crate::auth::RateLimiter>,
    pub chunk_size: u64,
}

impl AppState {
    pub fn new(pool: SqlitePool, data_dir: String, chunk_size: u64) -> Self {
        Self {
            pool,
            data_dir: Arc::new(data_dir),
            limiter: Arc::new(crate::auth::RateLimiter::new()),
            chunk_size,
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    use crate::handlers_auth as a;
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/register", post(a::register))
        .route("/auth/login", post(a::login))
        .route("/auth/logout", post(a::logout))
        .route("/api/me", get(a::me))
        .route("/api/admin/users", post(a::admin_create))
        .layer(middleware::from_fn(origin_guard))
        .with_state(state)
}

// CSRF: prima regola Sec-Fetch-Site (browser moderni, ammessi same-origin e none);
// ripiego per chi non lo manda: Origin assente → ok, presente → host deve coincidere
// con X-Forwarded-Host (proxy fidato) o Host. Da verificare sul gateway con tailscale serve.
async fn origin_guard(req: Request, next: Next) -> Response {
    if !matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return next.run(req).await;
    }
    let headers = req.headers();
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site != "same-origin" && site != "none" {
            return origin_bad();
        }
    } else if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let host = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim_end_matches('/');
        let origin_host = origin
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        if host != origin_host {
            return origin_bad();
        }
    }
    next.run(req).await
}

fn origin_bad() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": "origin non valido"})),
    )
        .into_response()
}

pub fn run() {
    let cfg = config::load();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        std::fs::create_dir_all(format!("{}/storage", cfg.data_dir)).unwrap();
        std::fs::create_dir_all(format!("{}/tmp", cfg.data_dir)).unwrap();
        let pool = db::new_pool(&format!("{}/vaulti.db", cfg.data_dir))
            .await
            .unwrap();
        db::migrate(&pool).await.unwrap();
        let state = AppState::new(pool, cfg.data_dir.clone(), cfg.chunk_size);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port))
            .await
            .unwrap();
        println!("vaulti sul port {}", cfg.port);
        axum::serve(listener, build_router(state)).await.unwrap();
    });
}
