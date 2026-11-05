use axum::http::HeaderName;
use axum::http::header::CONTENT_TYPE;
use axum::response::Html;

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}

pub async fn appjs() -> ([(HeaderName, &'static str); 1], &'static str) {
    (
        [(CONTENT_TYPE, "application/javascript")],
        include_str!("../../web/app.js"),
    )
}

pub async fn style() -> ([(HeaderName, &'static str); 1], &'static str) {
    ([(CONTENT_TYPE, "text/css")], include_str!("../../web/style.css"))
}
