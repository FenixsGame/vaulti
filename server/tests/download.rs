mod common;

use axum::http::StatusCode;
use common::*;
use tower::ServiceExt;

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
