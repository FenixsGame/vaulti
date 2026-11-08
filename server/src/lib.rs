pub mod auth;
pub mod config;
pub mod db;
pub mod handlers_auth;
pub mod handlers_files;
pub mod handlers_static;
pub mod handlers_store;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::delete;
use axum::routing::get;
use axum::routing::patch;
use axum::routing::post;
use axum::routing::put;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<PathBuf>,
    pub limiter: Arc<crate::auth::RateLimiter>,
    pub chunk_size: u64,
}

impl AppState {
    pub fn new(pool: SqlitePool, data_dir: PathBuf, chunk_size: u64) -> Self {
        Self {
            pool,
            data_dir: Arc::new(data_dir),
            limiter: Arc::new(crate::auth::RateLimiter::new()),
            chunk_size,
        }
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.data_dir.join("tmp")
    }

    pub fn upload_dir(&self, id: &str) -> PathBuf {
        self.data_dir.join("tmp").join(id)
    }

    pub fn storage_dir(&self, uid: &str) -> PathBuf {
        self.data_dir.join("storage").join(uid)
    }

    pub fn blob_path(&self, uid: &str, stored: &str) -> PathBuf {
        self.storage_dir(uid).join(stored)
    }
}

pub fn build_router(state: AppState) -> Router {
    use crate::handlers_auth as a;
    use crate::handlers_files as f;
    use crate::handlers_static as s;
    use crate::handlers_store as u;
    // limite body SOLO sui chunk; la 413 "attesa" nei test arriva dall'handler
    let chunks = Router::new()
        .route("/api/upload/{id}/chunk/{n}", put(u::chunk))
        .layer(DefaultBodyLimit::max(
            (state.chunk_size + 1024 * 1024) as usize,
        ));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/", get(s::index))
        .fallback(s::asset)
        .route("/auth/register", post(a::register))
        .route("/auth/login", post(a::login))
        .route("/auth/logout", post(a::logout))
        .route("/api/me", get(a::me))
        .route("/api/admin/users", post(a::admin_create))
        .route("/api/upload/start", post(u::start))
        .route("/api/upload/{id}/status", get(u::status))
        .route("/api/upload/{id}/finish", post(u::finish))
        .route("/api/upload/{id}", delete(u::cancel))
        .route("/api/files", get(f::list))
        .route("/api/files/{id}/download", get(f::download))
        .route(
            "/api/files/{id}",
            patch(f::patch_file).delete(f::delete_file),
        )
        .route("/api/folders", post(f::create_folder))
        .route(
            "/api/folders/{id}",
            patch(f::patch_folder).delete(f::delete_folder),
        )
        .merge(chunks)
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
        std::fs::create_dir_all(cfg.data_dir.join("storage")).unwrap();
        std::fs::create_dir_all(cfg.data_dir.join("tmp")).unwrap();
        let dbp = cfg.data_dir.join("vaulti.db");
        let pool = db::new_pool(dbp.to_str().unwrap()).await.unwrap();
        db::migrate(&pool).await.unwrap();
        let state = AppState::new(pool, cfg.data_dir.clone(), cfg.chunk_size);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port))
            .await
            .unwrap();
        println!("vaulti sul port {}", cfg.port);
        axum::serve(listener, build_router(state)).await.unwrap();
    });
}

/// Campo di emergenza: `vaulti-server reset-password <email>` — la nuova password
/// legge SOLO da stdin (mai argv, non finisce nella history dei processi);
/// invalida tutte le sessioni dell'utente.
pub fn reset_password() {
    let cfg = config::load();
    let Some(email) = std::env::args().nth(2) else {
        eprintln!("uso: vaulti-server reset-password <email>  (password da stdin)");
        std::process::exit(2);
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let dbp = cfg.data_dir.join("vaulti.db");
        let pool = db::new_pool(dbp.to_str().unwrap()).await.unwrap();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            eprintln!("password assente: serve una riga su stdin");
            std::process::exit(1);
        }
        let pw = line.trim_end_matches(['\n', '\r']).to_string();
        drop(line);
        if pw.len() < 8 {
            eprintln!("password troppo corta (minimo 8)");
            std::process::exit(1);
        }
        let hash = tokio::task::spawn_blocking(move || auth::hash_sync(&pw))
            .await
            .unwrap();
        let n = sqlx::query("UPDATE users SET password_hash = ?1 WHERE email = ?2")
            .bind(&hash)
            .bind(&email)
            .execute(&pool)
            .await
            .unwrap()
            .rows_affected();
        if n == 0 {
            eprintln!("utente non trovato: {email}");
            std::process::exit(1);
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE email = ?)")
            .bind(&email)
            .execute(&pool)
            .await
            .unwrap();
        println!("password aggiornata e sessioni invalidate per {email}");
    });
}
