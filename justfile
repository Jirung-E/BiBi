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

# Run the local checks shared with CI, including a debug build and a mock service smoke test.
test: build-debug
    cargo fmt --all --check
    npm --prefix gui/frontend run check
    npm --prefix gui/frontend test
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    node scripts/verify-service.mjs

# Install development dependencies from the lockfiles (does not install the app).
install:
    npm --prefix gui/frontend ci
    cargo fetch --locked

# Build and open the desktop app.
run:
    cargo run --release --locked
