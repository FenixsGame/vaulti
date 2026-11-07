// embed della cartella web/: MIME per estensione, no-cache + nosniff su tutto,
// fallback GET-only con difese anti-path-traversal (niente "..", niente backslash)
use axum::extract::Request;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web"]
struct Ui;

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "html" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

pub async fn index() -> Response {
    match Ui::get("index.html") {
        Some(f) => asset_response("text/html; charset=utf-8", f.data.into_owned()),
        None => miss(),
    }
}

pub async fn asset(req: Request) -> Response {
    if req.method() != Method::GET {
        return miss();
    }
    let path = req.uri().path().trim_start_matches('/');
    // solo file dentro web/: niente "..", niente backslash, mai il vuoto
    if path.is_empty() || path.contains("..") || path.contains('\\') {
        return miss();
    }
    match Ui::get(path) {
        Some(f) => asset_response(mime(path), f.data.into_owned()),
        None => miss(),
    }
}

fn asset_response(ct: &'static str, bytes: Vec<u8>) -> Response {
    (
        [
            (CONTENT_TYPE, ct),
            (CACHE_CONTROL, "no-cache"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

fn miss() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(CONTENT_TYPE, "text/plain"), (CACHE_CONTROL, "no-cache")],
        "",
    )
        .into_response()
}
