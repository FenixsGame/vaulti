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
    put_chunk(&t, &cookie, &id, 1, b"cd").await;
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
        .clone()
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
        .clone()
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

// ---- Task 5: cartelle + list/quota + delete ----

#[tokio::test]
async fn list_files_and_folders_and_quota() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    // cartella Prova
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "POST",
            "/api/folders",
            Some(serde_json::json!({"name": "Prova"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let fol: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    let fol_id = fol["id"].as_str().unwrap().to_string();
    // file dentro "Prova"
    let id = start_upload(&t, &cookie, "doc.txt", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"folder_id": fol_id})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    // list root → 1 subfolder pieno + 0 file
    let res = t
        .app
        .clone()
        .oneshot(authed_req("GET", "/api/files", None, &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["folders"].as_array().unwrap().len(), 1);
    assert_eq!(j["files"].as_array().unwrap().len(), 0);
    assert_eq!(j["quota"]["used"], 2);
    // list solo in "Prova" → 1 file con nome
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "GET",
            &format!("/api/files?folder_id={fol_id}"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["files"][0]["name"], "doc.txt");
    // rename file
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"name": "rin.doc.txt"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let nm: String = sqlx::query_scalar("SELECT name FROM files WHERE id = ?")
        .bind(&fid)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(nm, "rin.doc.txt");
}

#[tokio::test]
async fn delete_hard_removes_row_and_blob() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let (uid, stored): (String, String) =
        sqlx::query_as("SELECT f.user_id, f.stored_name FROM files f WHERE f.id = ?")
            .bind(&fid)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    let blob = std::fs::read(blob_path(&t, &uid, &stored)).unwrap();
    assert_eq!(blob, b"ab");
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/files/{fid}"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(
        !t.data_dir
            .path()
            .join("storage")
            .join(&uid)
            .join(&stored)
            .exists()
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn folder_delete_empty_only_cycles_and_dups() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let f1 = post_folder(&t, &cookie, "c1", None).await;
    assert_eq!(f1.1, 200);
    // duplicato nella stessa cartella → 409
    assert_eq!(post_folder(&t, &cookie, "c1", None).await.1, 409);
    // stesso nome in un'altra cartella è legale ("c1" dentro f2)
    let f2 = post_folder(&t, &cookie, "c2", None).await;
    let sub = post_folder(&t, &cookie, "c1", Some(&f2.0)).await;
    assert_eq!(sub.1, 200);
    // subfolder dentro f1: così DELETE f1 → 409 (non vuota)
    let sub1 = post_folder(&t, &cookie, "c1b", Some(&f1.0)).await;
    assert_eq!(sub1.1, 200);
    // f3 dentro f2 → 200; poi f2 dentro f3 (suo discendente) → 400; self → 400
    let f3 = post_folder(&t, &cookie, "c3", None).await;
    let ok = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/folders/{}", f3.0),
            Some(serde_json::json!({"parent_id": f2.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/folders/{}", f2.0),
            Some(serde_json::json!({"parent_id": f3.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/folders/{}", f2.0),
            Some(serde_json::json!({"parent_id": f2.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    // DELETE f1 → 409 (c1b dentro); f2 → 409 (f3 dentro); f3 → 409 (f2 parent... )
    // attenzione: f3 contiene il sub di f2? NO: f3 è IN f2, vuota? f3 vuota → 200
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/folders/{}", f1.0),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/folders/{}", f2.0),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/folders/{}", f3.0),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // parent inesistente → 404
    let resp = t
        .app
        .oneshot(authed_req(
            "POST",
            "/api/folders",
            Some(serde_json::json!({"name": "x", "parent_id": "no-such-folder"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

pub async fn post_folder(
    t: &TestApp,
    cookie: &str,
    name: &str,
    parent: Option<&str>,
) -> (String, StatusCode) {
    let body = match parent {
        Some(p) => serde_json::json!({"name": name, "parent_id": p}),
        None => serde_json::json!({"name": name}),
    };
    let res = t
        .app
        .clone()
        .oneshot(authed_req("POST", "/api/folders", Some(body), cookie))
        .await
        .unwrap();
    let st = res.status();
    let id = if st == StatusCode::OK {
        serde_json::from_slice::<Value>(&to_bytes(res.into_body()).await).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    } else {
        String::new()
    };
    (id, st)
}

// ---- Task 6: download + range + anti-xss ----

pub async fn make_file(t: &TestApp, cookie: &str) -> String {
    let id = start_upload(t, cookie, "cinema.mp4", 3).await;
    put_chunk(t, cookie, &id, 0, b"ab").await;
    put_chunk(t, cookie, &id, 1, b"c").await;
    finish_ok(t, cookie, &id).await
}

pub fn range_req(uri: &str, cookie: &str, range: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("cookie", cookie.to_string())
        .header("range", range)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn download_full_and_headers() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "GET",
            &format!("/api/files/{fid}/download"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let h = res.headers();
    let ct = h.get("content-type").unwrap().to_str().unwrap();
    assert_eq!(ct, "video/mp4");
    let cd = h.get("content-disposition").unwrap().to_str().unwrap();
    assert!(cd.starts_with("attachment"));
    assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
    let csp = h.get("content-security-policy").unwrap().to_str().unwrap();
    assert!(csp.contains("default-src 'none'"));
    let ar = h.get("accept-ranges").unwrap().to_str().unwrap();
    assert_eq!(ar, "bytes");
    assert_eq!(to_bytes(res.into_body()).await, b"abc");
}

#[tokio::test]
async fn download_range_206_truncated() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(range_req(
            &format!("/api/files/{fid}/download"),
            &cookie,
            "bytes=1-2",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(to_bytes(res.into_body()).await, b"bc");
}

#[tokio::test]
async fn download_range_end_beyond_size_truncated_206() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(range_req(
            &format!("/api/files/{fid}/download"),
            &cookie,
            "bytes=1-999999",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    let cr = res
        .headers()
        .get("content-range")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(cr, "bytes 1-2/3");
    assert_eq!(to_bytes(res.into_body()).await, b"bc");
}

#[tokio::test]
async fn download_range_suffix() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(range_req(
            &format!("/api/files/{fid}/download"),
            &cookie,
            "bytes=-2",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(to_bytes(res.into_body()).await, b"bc");
}

#[tokio::test]
async fn download_range_start_beyond_size_416() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(range_req(
            &format!("/api/files/{fid}/download"),
            &cookie,
            "bytes=5-",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let cr = res
        .headers()
        .get("content-range")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(cr, "bytes */3");
}

#[tokio::test]
async fn cross_user_file_404_and_deleted_404() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    t.app
        .clone()
        .oneshot(authed_req(
            "POST",
            "/api/admin/users",
            Some(serde_json::json!({"email": "b@b.c", "password": "secondpass"})),
            &cookie,
        ))
        .await
        .unwrap();
    let cookie2 = login_cookie(&t, "b@b.c", "secondpass").await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "GET",
            &format!("/api/files/{fid}/download"),
            None,
            &cookie2,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/files/{fid}"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = t
        .app
        .oneshot(authed_req(
            "GET",
            &format!("/api/files/{fid}/download"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn index_and_assets_served() {
    let t = boot().await;
    for uri in ["/", "/app.js", "/style.css"] {
        let res = t
            .app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!to_bytes(res.into_body()).await.is_empty());
    }
}

#[tokio::test]
async fn move_file_to_root_with_empty_folder_id() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fol = post_folder(&t, &cookie, "c", None).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"folder_id": fol.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"folder_id": ""})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let f: Option<Option<String>> = sqlx::query_scalar("SELECT folder_id FROM files WHERE id = ?")
        .bind(&fid)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert!(f.is_none());
}

#[tokio::test]
async fn download_name_control_chars_no_panic() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "a\nb.mp4", 3).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    put_chunk(&t, &cookie, &id, 1, b"c").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "GET",
            &format!("/api/files/{fid}/download"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let cd = res
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(cd.contains("a_b.mp4"));
    assert!(cd.contains("attachment"));
}

#[tokio::test]
async fn reset_password_cli_updates_hash_and_kills_sessions() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let dbp = format!("{}/vaulti.db", dir.path().to_string_lossy());
    let pool = vaulti_server::db::new_pool(&dbp).await.unwrap();
    vaulti_server::db::migrate(&pool).await.unwrap();
    let uid = uuid::Uuid::new_v4().to_string();
    let old = vaulti_server::auth::hash_sync("oldpassword");
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, is_admin, created_at) VALUES (?,?,?,?,?)",
    )
    .bind(&uid)
    .bind("a@b.c")
    .bind(&old)
    .bind(1)
    .bind(0)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ('deadbeef', ?, 0)")
        .bind(&uid)
        .execute(&pool)
        .await
        .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vaulti-server"))
        .args(["reset-password", "a@b.c"])
        .env("VAULTI_DATA_DIR", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"newpassword1\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ph: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE email = ?")
        .bind("a@b.c")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(vaulti_server::auth::verify_sync(&ph, "newpassword1"));
    assert!(!vaulti_server::auth::verify_sync(&ph, "oldpassword"));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = ?")
        .bind(&uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn version_flag_prints_version() {
    use std::process::Command;
    let out = Command::new(env!("CARGO_BIN_EXE_vaulti-server"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("vaulti-server {}", env!("CARGO_PKG_VERSION"))
    );
}
