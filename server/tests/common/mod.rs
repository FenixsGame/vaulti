use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

pub struct TestApp {
    pub app: axum::Router,
    pub pool: SqlitePool,
    pub data_dir: tempfile::TempDir,
}

pub async fn boot() -> TestApp {
    let data_dir = tempfile::tempdir().unwrap();
    let pool = vaulti_server::db::new_pool(":memory:").await.unwrap();
    vaulti_server::db::migrate(&pool).await.unwrap();
    let state = vaulti_server::AppState::new(pool.clone(), data_dir.path().to_path_buf(), 2);
    TestApp {
        app: vaulti_server::build_router(state),
        pool,
        data_dir,
    }
}

pub fn json_req(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-forwarded-for", "1.2.3.4")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn authed_req(method: &str, uri: &str, body: Option<Value>, cookie: &str) -> Request<Body> {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", cookie.to_string())
        .header("x-forwarded-for", "1.2.3.4");
    if let Some(v) = body {
        b.header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap()
    } else {
        b.body(Body::empty()).unwrap()
    }
}

pub fn raw_req(method: &str, uri: &str, cookie: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", cookie.to_string())
        .header("x-forwarded-for", "1.2.3.4")
        .body(Body::from(body))
        .unwrap()
}

pub async fn to_bytes(b: Body) -> Vec<u8> {
    b.collect().await.unwrap().to_bytes().to_vec()
}

pub async fn register_user(t: &TestApp, email: &str, pw: &str) {
    let res = t
        .app
        .clone()
        .oneshot(json_req(
            "/auth/register",
            serde_json::json!({"email": email, "password": pw}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

pub async fn login_cookie(t: &TestApp, email: &str, pw: &str) -> String {
    let res = t
        .app
        .clone()
        .oneshot(json_req(
            "/auth/login",
            serde_json::json!({"email": email, "password": pw}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

pub async fn logged_in(t: &TestApp) -> String {
    register_user(t, "a@b.c", "password123").await;
    login_cookie(t, "a@b.c", "password123").await
}

pub async fn start_upload(t: &TestApp, cookie: &str, name: &str, size: u64) -> String {
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "POST",
            "/api/upload/start",
            Some(serde_json::json!({"name": name, "size": size})),
            cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    j["upload_id"].as_str().unwrap().to_string()
}

pub async fn put_chunk(t: &TestApp, cookie: &str, id: &str, n: u32, data: &[u8]) -> StatusCode {
    let res = t
        .app
        .clone()
        .oneshot(raw_req(
            "PUT",
            &format!("/api/upload/{id}/chunk/{n}"),
            cookie,
            data.to_vec(),
        ))
        .await
        .unwrap();
    res.status()
}

pub async fn finish_ok(t: &TestApp, cookie: &str, up: &str) -> String {
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "POST",
            &format!("/api/upload/{up}/finish"),
            None,
            cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_slice::<Value>(&to_bytes(res.into_body()).await).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

// helper di dominio spostati qui dallo split di integration.rs

pub fn tmp_dir(t: &TestApp) -> String {
    format!("{}/tmp", t.data_dir.path().display())
}

pub fn blob_path(t: &TestApp, uid: &str, stored: &str) -> String {
    format!("{}/storage/{}/{}", t.data_dir.path().display(), uid, stored)
}

pub async fn finish_status(t: &TestApp, cookie: &str, id: &str) -> StatusCode {
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
