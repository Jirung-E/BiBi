use crate::{ApiError, AppState};
use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use bibi_core::{id, now};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn equal(a: &str, b: &str) -> bool {
    let a = Sha256::digest(a.as_bytes());
    let b = Sha256::digest(b.as_bytes());
    a.iter()
        .zip(b.iter())
        .fold(0u8, |diff, (x, y)| diff | (x ^ y))
        == 0
}
fn cookie(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|v| v.trim().strip_prefix("bibi_session="))
}
#[derive(Serialize, Deserialize)]
struct Session {
    expires_at: i64,
}
pub async fn guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !origin_allowed(&state, req.headers()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"허용되지 않은 출처입니다."})),
        )
            .into_response();
    }
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let valid_bearer = bearer.is_some_and(|token| equal(token, &state.token));
    let valid_cookie = cookie(req.headers()).is_some_and(|token| {
        state
            .store
            .setting::<Session>(&format!("session:{}", hash(token)))
            .ok()
            .flatten()
            .is_some_and(|s| s.expires_at > now())
    });
    if !valid_bearer && !valid_cookie {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"서버 인증이 필요합니다."})),
        )
            .into_response();
    }
    next.run(req).await
}
pub fn origin_allowed(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    if state.config.public_origin.as_deref() == Some(origin) {
        return true;
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    origin == format!("http://{host}") || origin == format!("https://{host}")
}
#[derive(Deserialize)]
pub struct Login {
    pub token: String,
}
pub async fn login(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<Login>,
) -> Result<Response, ApiError> {
    if !origin_allowed(&state, &headers) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "허용되지 않은 출처입니다.",
        ));
    }
    if !equal(&input.token, &state.token) {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "인증 토큰이 일치하지 않습니다.",
        ));
    }
    let session = format!("{}{}", id(""), id(""));
    state.store.set_setting(
        &format!("session:{}", hash(&session)),
        &Session {
            expires_at: now() + 30 * 24 * 3600 * 1000,
        },
    )?;
    let secure = if state.config.public_origin.is_some() {
        "; Secure"
    } else {
        ""
    };
    let value = format!(
        "bibi_session={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000{secure}"
    );
    Ok((
        [(header::SET_COOKIE, value)],
        Json(json!({"authenticated":true})),
    )
        .into_response())
}
pub async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    if !origin_allowed(&state, &headers) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "허용되지 않은 출처입니다.",
        ));
    }
    if let Some(token) = cookie(&headers) {
        state.store.set_setting(
            &format!("session:{}", hash(token)),
            &Session { expires_at: 0 },
        )?;
    }
    Ok((
        [(
            header::SET_COOKIE,
            "bibi_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Json(json!({"authenticated":false})),
    )
        .into_response())
}
