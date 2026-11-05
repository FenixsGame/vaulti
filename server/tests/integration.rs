mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
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
        .oneshot(authed_req(
            "POST",
            "/auth/logout",
            Some(serde_json::json!({})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
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

// ---- Task 4: upload preallocato ----

fn tmp_dir(t: &TestApp) -> String {
    format!("{}/tmp", t.data_dir.path().display())
}

fn blob_path(t: &TestApp, uid: &str, stored: &str) -> String {
    format!("{}/storage/{}/{}", t.data_dir.path().display(), uid, stored)
}

async fn finish_status(t: &TestApp, cookie: &str, id: &str) -> StatusCode {
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "POST",
            &format!("/api/upload/{id}/finish"),
            None,
            cookie,
        ))
        .await
        .unwrap();
    res.status()
}

#[tokio::test]
async fn upload_finish_chunks_in_order() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 5).await;
    assert_eq!(
        put_chunk(&t, &cookie, &id, 0, b"ab").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put_chunk(&t, &cookie, &id, 1, b"cd").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put_chunk(&t, &cookie, &id, 2, b"e").await,
        StatusCode::NO_CONTENT
    );
    finish_ok(&t, &cookie, &id).await;
    let (uid, stored): (String, String) = sqlx::query_as("SELECT user_id, stored_name FROM files")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    let blob = std::fs::read(blob_path(&t, &uid, &stored)).unwrap();
    assert_eq!(blob, b"abcde");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let d = std::fs::read_dir(tmp_dir(&t)).unwrap().count();
    assert_eq!(d, 0);
}

#[tokio::test]
async fn upload_multi_in_order() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let a = start_upload(&t, &cookie, "a.bin", 2).await;
    let b = start_upload(&t, &cookie, "b.bin", 3).await;
    assert_eq!(
        put_chunk(&t, &cookie, &a, 0, b"ab").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put_chunk(&t, &cookie, &b, 0, b"ab").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put_chunk(&t, &cookie, &b, 1, b"c").await,
        StatusCode::NO_CONTENT
    );
    finish_ok(&t, &cookie, &a).await;
    finish_ok(&t, &cookie, &b).await;
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM files ORDER BY name")
        .fetch_all(&t.pool)
        .await
        .unwrap();
    assert_eq!(names, vec!["a.bin", "b.bin"]);
}

#[tokio::test]
async fn finish_missing_chunk_409() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "m.bin", 5).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    put_chunk(&t, &cookie, &id, 1, b"cd").await;
    assert_eq!(finish_status(&t, &cookie, &id).await, StatusCode::CONFLICT);
    // le righe restano: il resume riprende dal chunk mancante
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn finish_last_chunk_wrong_len_409() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "l.bin", 5).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    put_chunk(&t, &cookie, &id, 1, b"cd").await;
    // l'ultimo chunk atteso è 1: "ef" è troppo lungo → 413 e nessuna riga
    assert_eq!(
        put_chunk(&t, &cookie, &id, 2, b"ef").await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(finish_status(&t, &cookie, &id).await, StatusCode::CONFLICT);
}

#[tokio::test]
async fn chunk_too_long_413() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "t.bin", 4).await;
    assert_eq!(
        put_chunk(&t, &cookie, &id, 0, b"abc").await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // il retry del medesimo chunk con la lunghezza giusta funziona
    assert_eq!(
        put_chunk(&t, &cookie, &id, 0, b"ab").await,
        StatusCode::NO_CONTENT
    );
    finish_ok(&t, &cookie, &id).await;
}

#[tokio::test]
async fn upload_zero_byte() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "e.bin", 0).await;
    finish_ok(&t, &cookie, &id).await;
    let (size,): (i64,) = sqlx::query_as("SELECT size FROM files")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(size, 0);
}

#[tokio::test]
async fn interrupted_chunk_not_marked_received() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "i.bin", 3).await;
    // body corto rispetto all'atteso → 400 e NESSUNA riga upload_chunks
    assert_eq!(
        put_chunk(&t, &cookie, &id, 0, b"a").await,
        StatusCode::BAD_REQUEST
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upload_chunks")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // il retry del medesimo chunk completo funziona (idempotenza)
    assert_eq!(
        put_chunk(&t, &cookie, &id, 0, b"ab").await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn status_lists_received_only() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "s.bin", 3).await;
    put_chunk(&t, &cookie, &id, 1, b"c").await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "GET",
            &format!("/api/upload/{id}/status"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["received"], serde_json::json!([1]));
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let res = t
        .app
        .oneshot(authed_req(
            "GET",
            &format!("/api/upload/{id}/status"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["received"], serde_json::json!([0, 1]));
}

#[tokio::test]
async fn chunk_wrong_owner_404() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "w.bin", 2).await;
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
    assert_eq!(res.status(), StatusCode::OK);
    let cookie2 = login_cookie(&t, "b@b.c", "secondpass").await;
    assert_eq!(
        put_chunk(&t, &cookie2, &id, 0, b"ab").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn chunk_invalid_id_path_not_written() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    assert_eq!(
        put_chunk(&t, &cookie, "not-a-uuid", 0, b"ab").await,
        StatusCode::NOT_FOUND
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upload_chunks")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn start_no_space_507() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let res = t
        .app
        .oneshot(authed_req(
            "POST",
            "/api/upload/start",
            Some(serde_json::json!({"name": "x.bin", "size": u64::MAX})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::INSUFFICIENT_STORAGE);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn cancel_upload_cleanup() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "c.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let res = t
        .app
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/upload/{id}"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upload_chunks")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let d = std::fs::read_dir(tmp_dir(&t)).unwrap().count();
    assert_eq!(d, 0);
}

#[tokio::test]
async fn start_sweeps_older_tmp() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let a = start_upload(&t, &cookie, "a.bin", 1).await;
    sqlx::query("UPDATE uploads SET created_at = -999999999")
        .execute(&t.pool)
        .await
        .unwrap();
    start_upload(&t, &cookie, "b.bin", 1).await;
    let b_id: String = sqlx::query_scalar("SELECT id FROM uploads")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_ne!(b_id, a);
    assert!(!t.data_dir.path().join("tmp").join(&a).exists());
}

#[tokio::test]
async fn finish_duplicate_name_409() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let a = start_upload(&t, &cookie, "a.bin", 1).await;
    put_chunk(&t, &cookie, &a, 0, b"a").await;
    finish_ok(&t, &cookie, &a).await;
    let b = start_upload(&t, &cookie, "a.bin", 1).await;
    put_chunk(&t, &cookie, &b, 0, b"a").await;
    assert_eq!(finish_status(&t, &cookie, &b).await, StatusCode::CONFLICT);
    // il blob originale resta intatto, un solo file, tmp pulito
    let (uid, stored): (String, String) = sqlx::query_as("SELECT user_id, stored_name FROM files")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(std::fs::read(blob_path(&t, &uid, &stored)).unwrap(), b"a");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let d = std::fs::read_dir(tmp_dir(&t)).unwrap().count();
    assert_eq!(d, 0);
}
