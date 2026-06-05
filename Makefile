# Colors (disabled when stdout is not a terminal: `make NO_COLOR=1` also works)
ifndef NO_COLOR
BOLD   := \033[1m
CYAN   := \033[36m
GREEN  := \033[32m
YELLOW := \033[33m
RESET  := \033[0m
endif

.DEFAULT_GOAL := help
.PHONY: help build test lint fmt fmt-check doc check clean install

help: ## List the most important targets
	@printf "$(BOLD)ytdown$(RESET) — workspace targets\n\n"
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "} {printf "  $(CYAN)%-10s$(RESET) %s\n", $$1, $$2}'

build: ## Build all crates with all features
	@printf "$(YELLOW)▶ build$(RESET) cargo build --workspace --all-features\n"
	@cargo build --workspace --all-features
	@printf "$(GREEN)✓ build$(RESET)\n"

test: ## Run the full workspace test suite
	@printf "$(YELLOW)▶ test$(RESET) cargo test --workspace --all-features\n"
	@cargo test --workspace --all-features
	@printf "$(GREEN)✓ test$(RESET)\n"

lint: ## Clippy with warnings denied
	@printf "$(YELLOW)▶ lint$(RESET) cargo clippy --workspace --all-targets --all-features -- -D warnings\n"
	@cargo clippy --workspace --all-targets --all-features -- -D warnings
	@printf "$(GREEN)✓ lint$(RESET)\n"

fmt: ## Format the whole workspace
	@printf "$(YELLOW)▶ fmt$(RESET) cargo fmt --all\n"
	@cargo fmt --all
	@printf "$(GREEN)✓ fmt$(RESET)\n"

fmt-check: ## Verify formatting without changing files
	@printf "$(YELLOW)▶ fmt-check$(RESET) cargo fmt --all -- --check\n"
	@cargo fmt --all -- --check
	@printf "$(GREEN)✓ fmt-check$(RESET)\n"

doc: ## Build API docs (no deps)
	@printf "$(YELLOW)▶ doc$(RESET) cargo doc --workspace --no-deps --all-features\n"
	@cargo doc --workspace --no-deps --all-features
	@printf "$(GREEN)✓ doc$(RESET)\n"

check: fmt-check lint test ## Full gate: fmt-check + lint + test (same as pre-commit)
	@printf "$(GREEN)✓ check — all gates passed$(RESET)\n"

install: ## Install the ytdown CLI from this checkout
	@printf "$(YELLOW)▶ install$(RESET) cargo install --path cli\n"
	@cargo install --path cli
	@printf "$(GREEN)✓ install$(RESET)\n"

clean: ## Remove build artifacts
	@printf "$(YELLOW)▶ clean$(RESET) cargo clean\n"
	@cargo clean
	@printf "$(GREEN)✓ clean$(RESET)\n"
