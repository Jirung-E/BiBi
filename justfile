set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# List available commands.
default:
    @just --list

# Build the frontend, CLI/server, and desktop binaries in release mode.
build:
    cargo run --release --locked -- --check

# Build the frontend, CLI/server, and desktop binaries in debug mode without opening the app.
build-debug:
    cargo run --locked -- --check

# Run the same host-platform checks locally and in CI, including release packaging.
test:
    node scripts/verify-logged.mjs target/verification/just-test.log node scripts/verify-source-tree.mjs just _test

# The public test recipe checks that this pipeline does not rewrite source files.
[private]
_test:
    node --test scripts/verify-logged.test.mjs
    cargo fmt --all --check
    node scripts/npm.mjs --prefix gui/frontend run check:ui
    node scripts/npm.mjs --prefix gui/frontend run check
    node scripts/npm.mjs --prefix gui/frontend test
    node scripts/npm.mjs --prefix gui/frontend run build
    node scripts/npm.mjs --prefix gui/frontend run test:ui
    just build-debug
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    node scripts/verify-service.mjs
    just build
    node scripts/verify-package.mjs --prebuilt

# Exercise the real NSIS install/uninstall lifecycle in a clean Windows account.
[windows]
test-install:
    node scripts/verify-logged.mjs target/verification/windows-install.log node scripts/verify-windows-install.mjs

# Install development dependencies from the lockfiles (does not install the app).
install:
    node scripts/install-dependencies.mjs
    node scripts/npm.mjs --prefix gui/frontend run install:ui
    cargo fetch --locked

# Build and open the desktop app.
run:
    cargo run --release --locked
