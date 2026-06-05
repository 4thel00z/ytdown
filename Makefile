.PHONY: build test lint fmt fmt-check doc check
build: ; cargo build --workspace --all-features
test: ; cargo test --workspace --all-features
lint: ; cargo clippy --workspace --all-targets --all-features -- -D warnings
fmt: ; cargo fmt --all
fmt-check: ; cargo fmt --all -- --check
doc: ; cargo doc --workspace --no-deps --all-features
check: fmt-check lint test
