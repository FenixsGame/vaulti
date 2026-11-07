// embed della cartella web/: MIME per estensione, no-cache + nosniff su tutto,
// fallback GET-only con difese anti-path-traversal (niente "..", niente backslash)
use axum::extract::Request;
use axum::http::Method;
use axum::http::StatusCode;
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
    // manifest e test non sono asset dell'interfaccia: mai serviti via fallback
    if path == "package.json" || path.ends_with(".test.mjs") {
        return miss();
    }
    match Ui::get(path) {
        Some(f) => asset_response(mime(path), f.data.into_owned()),
        None => miss(),
    }
}

fn asset_response(ct: &'static str, bytes: Vec<u8>) -> Response {
    // chiavi tutte &str: in un array-tuple axum unifica il tipo di ogni elemento
    (
        [
            ("content-type", ct),
            ("cache-control", "no-cache"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

fn miss() -> Response {
    // chiavi tutte &str come in asset_response: nello stesso array-tuple axum
    (
        StatusCode::NOT_FOUND,
        [
            ("content-type", "text/plain"),
            ("cache-control", "no-cache"),
            ("x-content-type-options", "nosniff"),
        ],
        "",
    )
        .into_response()
}
