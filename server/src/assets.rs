use crate::AppState;
use axum::{
    extract::State,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
pub async fn serve(
    State(state): State<AppState>,
    uri: Uri,
    request: axum::extract::Request,
) -> Response {
    if let Some(path) = &state.config.frontend {
        return ServeDir::new(path)
            .not_found_service(ServeFile::new(path.join("index.html")))
            .oneshot(request)
            .await
            .unwrap()
            .into_response();
    }
    let name = uri.path().trim_start_matches('/');
    let asset = ASSETS.iter().find(|(key, _)| *key == name).or_else(|| {
        if !name.rsplit('/').next().unwrap_or("").contains('.') {
            ASSETS.iter().find(|(key, _)| *key == "index.html")
        } else {
            None
        }
    });
    let Some((name, bytes)) = asset else {
        return (
            StatusCode::NOT_FOUND,
            "UI assets are unavailable. Build gui/frontend before compiling BiBi.",
        )
            .into_response();
    };
    let mime = match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let cache = if name.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        *bytes,
    )
        .into_response()
}
