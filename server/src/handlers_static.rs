use axum::http::HeaderName;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};

pub async fn index() -> ([(HeaderName, &'static str); 2], &'static str) {
    (
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../web/index.html"),
    )
}

pub async fn appjs() -> ([(HeaderName, &'static str); 2], &'static str) {
    (
        [
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../web/app.js"),
    )
}

pub async fn style() -> ([(HeaderName, &'static str); 2], &'static str) {
    (
        [
            (CONTENT_TYPE, "text/css; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../web/style.css"),
    )
}
