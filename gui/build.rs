fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let features = std::env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    if target == "msvc" && features.split(',').any(|feature| feature == "crt-static") {
        // Cargo links the complete static CRT. Tauri's partial override would
        // disable libucrt and conflict with that choice.
        // SAFETY: this build script is still single-threaded, before Tauri starts.
        unsafe { std::env::remove_var("STATIC_VCRUNTIME") };
    }
    tauri_build::build();
}
