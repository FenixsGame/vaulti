mod common;

use axum::body::Body;
use axum::http::Request;
use common::*;
use serde_json::Value;
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

#[tokio::test]
async fn register_first_user_then_403() {
    let t = boot().await;
    let res = t
        .app
        .clone()
        .oneshot(json_req(
            "/auth/register",
            serde_json::json!({"email": "a@b.c", "password": "password123"}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let res = t
        .app
        .oneshot(json_req(
            "/auth/register",
            serde_json::json!({"email": "d@e.f", "password": "passwordabc"}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn register_short_password_400() {
    let t = boot().await;
    let res = t
        .app
        .oneshot(json_req(
            "/auth/register",
            serde_json::json!({"email": "a@b.c", "password": "pw"}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn login_ok_cookie_and_me_bool_admin() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let cookie = login_cookie(&t, "a@b.c", "password123").await;
    assert!(cookie.starts_with("session=") && cookie.len() > 40);
    let res = t
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["email"], "a@b.c");
    assert_eq!(j["is_admin"], true);
}

#[tokio::test]
async fn login_bad_password_401() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let res = t
        .app
        .clone()
        .oneshot(json_req(
            "/auth/login",
            serde_json::json!({"email": "a@b.c", "password": "nope"}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn me_without_session_401() {
    let t = boot().await;
    let res = t
        .app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn session_expired_401() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let cookie = login_cookie(&t, "a@b.c", "password123").await;
    sqlx::query("UPDATE sessions SET expires_at = -1")
        .execute(&t.pool)
        .await
        .unwrap();
    let res = t
        .app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn login_ratelimit_429() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let app = t.app;
    for _ in 0..5 {
        let res = app
            .clone()
            .oneshot(json_req(
                "/auth/login",
                serde_json::json!({"email": "a@b.c", "password": "x"}),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), 401);
    }
    let res = app
        .oneshot(json_req(
            "/auth/login",
            serde_json::json!({"email": "a@b.c", "password": "x"}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 429);
}

#[tokio::test]
async fn logout_invalidates() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let res = t
        .app
        .clone()
        .oneshot(json_req("/auth/logout", serde_json::json!({})))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let res = t
        .app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn admin_can_create_second_user() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "POST",
            "/api/admin/users",
            Some(serde_json::json!({"email": "b@b.c", "password": "secondpass"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let cookie2 = login_cookie(&t, "b@b.c", "secondpass").await;
    let res = t
        .app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("cookie", cookie2)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["is_admin"], false);
}
