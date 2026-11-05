pub mod auth;
pub mod config;
pub mod db;

use axum::{Router, routing::get};
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

pub fn build_router(_state: AppState) -> Router {
    Router::new().route("/healthz", get(|| async { "ok" }))
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
