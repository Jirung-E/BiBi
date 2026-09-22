pub mod model;
mod store;

pub use model::*;
pub use store::{Error, Result, Store};

pub const PRODUCT_NAME: &str = "BiBi";
pub const COMMAND_NAME: &str = "bibi";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub fn id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

pub fn arrow_opacity(sent_at: i64, now: i64, half_life: f64, floor: f64) -> f64 {
    let age = (now - sent_at).max(0) as f64 / 1000.0;
    floor.clamp(0.0, 1.0) + (1.0 - floor.clamp(0.0, 1.0)) * 2f64.powf(-age / half_life.max(1.0))
}
