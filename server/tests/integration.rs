mod common;

use axum::body::Body;
use axum::http::Request;
use common::*;
use tower::ServiceExt;

#[tokio::test]
async fn healthz_ok() {
    let t = boot().await;
    let res = t
        .app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn migrations_create_tables() {
    let t = boot().await;
    for table in [
        "users",
        "sessions",
        "folders",
        "files",
        "uploads",
        "upload_chunks",
    ] {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&t.pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "tabella {table} manca");
    }
}
