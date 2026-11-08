mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use tower::ServiceExt;

#[tokio::test]
async fn index_and_assets_served() {
    let t = boot().await;
    for uri in ["/", "/css/base.css", "/js/app.js"] {
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
async fn static_assets_no_cache() {
    let t = boot().await;
    for path in ["/", "/css/base.css", "/js/app.js"] {
        let res = t
            .app
            .clone()
            .oneshot(raw_req("GET", path, "", Vec::new()))
            .await
            .unwrap();
        assert_eq!(
            res.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-cache"),
            "route {path}"
        );
    }
}

#[tokio::test]
async fn asset_js_has_js_mime() {
    let t = boot().await;
    let res = t
        .app
        .clone()
        .oneshot(raw_req("GET", "/js/app.js", "", Vec::new()))
        .await
        .unwrap();
    assert_eq!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/javascript; charset=utf-8")
    );
}

#[tokio::test]
async fn asset_missing_is_404() {
    let t = boot().await;
    for path in [
        "/nope",
        "/../x%2Fx",
        "/../css/base.css",
        "/package.json",
        "/js/format.test.mjs",
    ] {
        let res = t
            .app
            .clone()
            .oneshot(raw_req("GET", path, "", Vec::new()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "route {path}");
    }
}

#[tokio::test]
async fn post_to_fallback_is_404() {
    let t = boot().await;
    let res = t
        .app
        .clone()
        .oneshot(raw_req("POST", "/css/base.css", "", Vec::new()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
