set windows-shell := ["cmd.exe", "/d", "/s", "/c"]

# List available commands.
default:
    @just --list

# Build the frontend, CLI/server, and desktop binaries in release mode.
build:
    cargo run --release --locked -- --check

# Build and open the desktop app.
run:
    cargo run --release --locked
