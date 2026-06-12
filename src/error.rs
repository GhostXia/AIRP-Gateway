//! Unified error type for the gateway, with an axum [`IntoResponse`] mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub type Result<T> = std::result::Result<T, GatewayError>;

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("unknown upstream: {0}")]
    UnknownUpstream(String),

    #[error("no route matched: {0} {1}")]
    NoRoute(String, String),

    #[error("upstream transport error: {0}")]
    Transport(String),

    #[error("upstream returned JSON-RPC error {code}: {message}")]
    Upstream { code: i64, message: String },

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    /// A code path that is scaffolded but not yet implemented.
    #[error("not implemented: {0}")]
    Unimplemented(&'static str),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl GatewayError {
    pub fn status(&self) -> StatusCode {
        match self {
            GatewayError::Unauthorized => StatusCode::UNAUTHORIZED,
            GatewayError::BadRequest(_) => StatusCode::BAD_REQUEST,
            GatewayError::NoRoute(_, _) => StatusCode::NOT_FOUND,
            GatewayError::UnknownUpstream(_) => StatusCode::BAD_GATEWAY,
            GatewayError::Transport(_) | GatewayError::Upstream { .. } => StatusCode::BAD_GATEWAY,
            GatewayError::Unimplemented(_) => StatusCode::NOT_IMPLEMENTED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(json!({
            "error": {
                "type": status.canonical_reason().unwrap_or("error"),
                "message": self.to_string(),
            }
        }));
        (status, body).into_response()
    }
}
