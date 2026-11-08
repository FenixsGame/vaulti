use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};

pub struct AppError(StatusCode, String);

impl AppError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error": self.1}))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(_: sqlx::Error) -> Self {
        // niente detagli SQL verso fuori (fingerprint, path del DB ecc.)
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "errore interno")
    }
}

// UNIQUE → 409 col messaggio di dominio; qualsiasi altro errore SQL → 500
pub fn unique_conflict(e: sqlx::Error, msg: &'static str) -> AppError {
    if e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
    {
        AppError::new(StatusCode::CONFLICT, msg)
    } else {
        e.into()
    }
}
