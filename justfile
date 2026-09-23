set windows-shell := ["cmd.exe", "/d", "/s", "/c"]

# List available commands.
default:
    @just --list

# Build the frontend, CLI/server, and desktop binaries in release mode.
build:
    cargo run --release --locked -- --check

# Build the frontend, CLI/server, and desktop binaries in debug mode without opening the app.
build-debug:
    cargo run --locked -- --check

# Check UI before the heavier desktop build, then verify the mock service.
test:
    cargo fmt --all --check
    npm --prefix gui/frontend run check:ui
    npm --prefix gui/frontend run check
    npm --prefix gui/frontend test
    npm --prefix gui/frontend run build
    npm --prefix gui/frontend run test:ui
    just build-debug
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    node scripts/verify-service.mjs

# Install development dependencies from the lockfiles (does not install the app).
install:
    npm --prefix gui/frontend ci
    npm --prefix gui/frontend run install:ui
    cargo fetch --locked

# Build and open the desktop app.
run:
    cargo run --release --locked
