mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use common::{TestApp, boot, boot_origin};

// POST /auth/register: su boot vergine l'utente 0 → solo il guard può dare 403
fn register_req(headers: &[(&str, &str)]) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/auth/register")
        .header("content-type", "application/json");
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(Body::from(
        serde_json::json!({"email": "a@b.c", "password": "password123"}).to_string(),
    ))
    .unwrap()
}

async fn post(t: &TestApp, headers: &[(&str, &str)]) -> StatusCode {
    t.app
        .clone()
        .oneshot(register_req(headers))
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn sec_fetch_cross_site_rejected() {
    let t = boot().await;
    let st = post(
        &t,
        &[
            ("sec-fetch-site", "cross-site"),
            ("origin", "https://evil.com"),
        ],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn foreign_origin_rejected() {
    let t = boot_origin("https://box.ts.net").await;
    // coppia Origin/XFH (entrambe controllabili dall'attaccante) → deve valere la config
    let st = post(
        &t,
        &[
            ("origin", "https://evil.com"),
            ("x-forwarded-host", "evil.com"),
        ],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn matching_origin_accepted() {
    let t = boot_origin("https://box.ts.net").await;
    let st = post(&t, &[("origin", "https://box.ts.net")]).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn no_origin_headers_passed() {
    // documentato: curl/CLI/desktop senza Sec-Fetch-Site né Origin passano
    let t = boot().await;
    let st = post(&t, &[]).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn xfh_pair_without_config_rejected() {
    // regressione del buco segnalato nella review S2: senza VAULTI_ORIGIN il
    // fallback è Host e SOLO Host — Origin+XFH uguali non aprono più la porta
    let t = boot().await;
    let st = post(
        &t,
        &[
            ("origin", "https://evil.com"),
            ("x-forwarded-host", "evil.com"),
            ("host", "localhost:8420"),
        ],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
