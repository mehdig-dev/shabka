use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};

/// Application error type that renders as an HTML error page.
pub struct AppError(pub anyhow::Error);

impl AppError {
    /// Check if the error is likely a HelixDB connectivity issue.
    fn is_db_unavailable(&self) -> bool {
        let msg = format!("{:#}", self.0).to_lowercase();
        msg.contains("connection refused")
            || msg.contains("timed out")
            || msg.contains("connect error")
            || msg.contains("dns error")
            || msg.contains("no connection")
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("web error: {:#}", self.0);

        if self.is_db_unavailable() {
            let body = r#"<!doctype html>
<html><head><title>Database Unavailable — Kaizen</title>
<style>body{font-family:system-ui;background:#0f0f1a;color:#e0e0e0;display:flex;justify-content:center;align-items:center;height:100vh;margin:0}
.box{text-align:center;max-width:500px}
h1{font-size:2.5rem;color:#f39c12;margin:0}
p{color:#888;margin:0.5rem 0}
code{background:#1a1a2e;padding:0.2rem 0.5rem;border-radius:4px;color:#6c63ff}
.retry{margin-top:1.5rem}
a{color:#6c63ff;text-decoration:none;padding:0.5rem 1rem;border:1px solid #2a2a4a;border-radius:8px}
a:hover{border-color:#6c63ff;background:rgba(108,99,255,0.1)}</style>
</head><body><div class="box"><h1>Database Unavailable</h1>
<p>Cannot connect to HelixDB. Make sure it's running:</p>
<p><code>just db</code></p>
<div class="retry"><a href="javascript:location.reload()">Retry</a></div>
</div></body></html>"#;
            return (StatusCode::SERVICE_UNAVAILABLE, Html(body.to_string())).into_response();
        }

        let body = format!(
            r#"<!doctype html>
<html><head><title>Error</title>
<style>body{{font-family:system-ui;background:#1a1a2e;color:#e0e0e0;display:flex;justify-content:center;align-items:center;height:100vh;margin:0}}
.err{{background:#16213e;padding:2rem;border-radius:8px;border-left:4px solid #e74c3c;max-width:600px}}
h1{{color:#e74c3c;margin-top:0}}pre{{white-space:pre-wrap;color:#aaa}}</style>
</head><body><div class="err"><h1>Something went wrong</h1><pre>{}</pre>
<p><a href="/" style="color:#3498db">Back to home</a></p></div></body></html>"#,
            html_escape(&format!("{:#}", self.0))
        );
        (StatusCode::INTERNAL_SERVER_ERROR, Html(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

/// JSON API error type for REST endpoints.
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        tracing::error!("api error: {:#}", err);
        Self::internal(format!("{:#}", err))
    }
}

impl From<kaizen_core::error::KaizenError> for ApiError {
    fn from(err: kaizen_core::error::KaizenError) -> Self {
        match &err {
            kaizen_core::error::KaizenError::NotFound(_) => Self::not_found(err.to_string()),
            kaizen_core::error::KaizenError::InvalidInput(_) => Self::bad_request(err.to_string()),
            _ => {
                tracing::error!("api error: {}", err);
                Self::internal(err.to_string())
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
