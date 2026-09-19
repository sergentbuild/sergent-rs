# Bare `just` errors; developer must specify a recipe name.
# No quotes around the echo arg so cmd.exe doesn't echo the literal quotes.
[private]
default:
    @echo "ERROR: no recipe specified"
    @exit 1

set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]

# Run the full Rust workspace test suite
test:
    cargo test --workspace --quiet

# Format all Rust code in place
fmt:
    cargo fmt --all

# Lint gate: format check, clippy (warnings denied)
lint:
    @cargo fmt --all --check
    @cargo check --all-targets
    @cargo clippy --workspace --all-targets -- -D warnings
