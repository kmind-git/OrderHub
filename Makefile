# Local OrderHub development gates. All required Rust gates use rust-toolchain.toml.
SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
CARGO_FEATURES := $(shell bash scripts/cargo-features.bash)
CHECK = cargo run --locked --profile $(PROFILE) -p orderhub-tools --no-default-features --features checks --bin orderhub-check --
ORDERHUB := -p orderhub-gateway -p orderhub-node -p orderhub-proto -p orderhub-tools
CORE_CRATES := nautilus-common nautilus-core nautilus-cryptography nautilus-data \
    nautilus-execution nautilus-infrastructure nautilus-macros nautilus-model \
    nautilus-portfolio nautilus-risk nautilus-testkit \
    orderhub-gateway orderhub-node orderhub-tools
ADAPTER_CRATES :=
NO_TEST_CRATES := orderhub-proto
PROFILE ?= nextest
OUTPUT ?= target/orderhub-db-package

.PHONY: help format cargo-fmt check cargo-check check-all-targets lint cargo-clippy test cargo-test \
    cargo-test-core test-scripts pre-flight pre-commit docs docs-rust check-dependency-features \
    check-test-network check-workspace-test-coverage check-retired-surfaces schema-check package-db-tool \
    security-audit security-check-tools security-validate cargo-clippy-strict markdown tables doc-links \
    cargo-test-doc cargo-update outdated cargo-build cargo-build-release

help:
	@echo 'OrderHub: format, check, lint, test, pre-flight, docs, schema-check, package-db-tool, security-audit'
	@echo 'Retained OMS libraries: cargo-test-core; DB service tests are activated separately.'

format:
	cargo fmt --all
cargo-fmt:
	cargo fmt --all -- --check
check cargo-check check-all-targets:
	cargo check --locked --profile $(PROFILE) --workspace --all-targets
lint cargo-clippy:
	cargo clippy --locked --profile $(PROFILE) --workspace --all-targets -- -D warnings
test cargo-test:
	bash scripts/strip-adapter-env.bash cargo test --locked $(ORDERHUB) --profile $(PROFILE) --lib --bins
cargo-test-core:
	bash scripts/strip-adapter-env.bash cargo test --locked --workspace --profile $(PROFILE) --lib --bins
cargo-test-doc:
	cargo test --locked --profile $(PROFILE) --workspace --doc
pre-flight: cargo-fmt check lint check-dependency-features check-test-network check-workspace-test-coverage check-retired-surfaces security-validate test
pre-commit:
	prek run --all-files
check-dependency-features:
	$(CHECK) dependency-features
check-test-network:
	$(CHECK) test-network
check-workspace-test-coverage:
	$(CHECK) workspace-tests
check-retired-surfaces:
	$(CHECK) retired-surfaces
test-scripts:
	cargo test --locked -p orderhub-tools --no-default-features --features checks --profile $(PROFILE) --bin orderhub-check
	bash scripts/test-native-path.bash
	bash scripts/test-check-cargo-cooldown.bash
	bash scripts/test-update-cargo-dependencies.bash
	bash scripts/ci/test-tool-version-scripts.bash
	bash scripts/orderhub/test-package-centos9.bash
schema-check:
	cargo run --locked --profile $(PROFILE) -p orderhub-tools --bin orderhub-db -- schema-check --schema schema/sql
package-db-tool:
	bash scripts/orderhub/package-db-tool.bash '$(OUTPUT)'
security-validate:
	$(CHECK) security validate
security-check-tools:
	$(CHECK) security check-tools
security-audit:
	$(CHECK) security run
# Optional stricter diagnostics; findings remain reports rather than a new acceptance gate.
cargo-clippy-strict:
	@mkdir -p target/audit
	cargo clippy --locked --profile $(PROFILE) --workspace --lib --bins --tests --no-deps --message-format=json -- \
	    --force-warn clippy::arithmetic_side_effects --force-warn clippy::as_conversions \
	    --force-warn clippy::expect_used --force-warn clippy::indexing_slicing \
	    --force-warn clippy::unwrap_used --force-warn clippy::panic --force-warn clippy::string_slice \
	    --force-warn clippy::unreachable --force-warn clippy::todo --force-warn clippy::unimplemented \
	    --force-warn clippy::exit --force-warn clippy::panic_in_result_fn \
	    --force-warn clippy::unchecked_time_subtraction > target/audit/clippy-strict.jsonl
docs docs-rust:
	cargo doc --locked --profile $(PROFILE) --workspace --no-deps
markdown:
	markdownlint-cli2 --config .markdownlint.jsonc
tables:
	git ls-files -z -- '*.md' | xargs -0 $(CHECK) markdown-tables --display-width
doc-links:
	$(CHECK) doc-links target/doc-links.md
	lychee --offline target/doc-links.md
cargo-build:
	cargo build --locked --profile $(PROFILE) $(ORDERHUB)
cargo-build-release:
	cargo build --locked $(ORDERHUB) --release
cargo-update:
	bash scripts/update-cargo-dependencies.bash
outdated:
	bash scripts/check-outdated.sh
