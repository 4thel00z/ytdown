.PHONY: build test lint fmt fmt-check doc check
build: ; cargo build --all-features
test: ; cargo test --all-features
lint: ; cargo clippy --all-targets --all-features -- -D warnings
fmt: ; cargo fmt --all
fmt-check: ; cargo fmt --all -- --check
doc: ; cargo doc --no-deps --all-features
check: fmt-check lint test
