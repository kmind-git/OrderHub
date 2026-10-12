# Contributing to OrderHub

OrderHub is maintained independently at [kmind-git/OrderHub](https://github.com/kmind-git/OrderHub).
Coordinate issues, changes, and releases with OrderHub's maintainers. The default branch is
`master`; existing internal `nautilus-*` crate and API names remain compatibility identifiers.

Live trading errors can cause financial loss. Review changes to admission, risk, execution,
persistence, recovery, and reconciliation against that consequence.

## Set up the environment

Use the pinned stable Rust toolchain in `rust-toolchain.toml`. Supported local tools require no
Python environment. See [environment setup](docs/developer_guide/environment_setup.md) for
platform prerequisites.

The Rust workspace is under `crates/`. The OrderHub crates are `orderhub-gateway`,
`orderhub-node`, and `orderhub-proto`. Existing `nautilus-*` crates remain internal dependencies.

## Make and verify a change

Keep changes focused and include tests when behavior changes. Run focused checks first. For
OrderHub Rust changes, use the stable toolchain pinned in `rust-toolchain.toml` with its `rustfmt`
and `clippy` components, then use:

```bash
cargo fmt --all -- --check
cargo check --locked --profile nextest -p orderhub-gateway -p orderhub-node -p orderhub-proto -p orderhub-tools --all-targets
cargo clippy --locked --profile nextest -p orderhub-gateway -p orderhub-node -p orderhub-proto -p orderhub-tools --all-targets -- -D warnings
cargo test --locked --profile nextest -p orderhub-gateway -p orderhub-node -p orderhub-proto -p orderhub-tools --lib --bins
```

The nextest profile disables debug symbols and incremental caches to limit validation disk use.

The repository also provides `make format`, `make pre-commit`, and broader test targets. Run all
checks relevant to the files changed before calling a change ready. Where a required check cannot
run, state why and what remains unverified. Repeat affected checks after a later edit or rebase.

The dedicated PostgreSQL schema tool is documented in
[OrderHub tooling](docs/orderhub/tooling.md). Its `schema-check` command runs without a service;
database initialization and repeat-migration behavior require a dedicated PostgreSQL test instance
when that integration is enabled. The same crate provides `orderhub-check`; its
`dependency-features` and `test-network` checks run without Python or a database service.

Follow [coding standards](docs/developer_guide/coding_standards.md). Preserve file headers and
license notices. Update generated files through their generator. Keep Redis and PostgreSQL test
coverage when changing those integrations.

The historical converter is an optional offline entry. For changes to it, run formatting,
Clippy, and its library tests with `--no-default-features --features legacy-catalog`, including
the original 64/128-bit Parquet compatibility tests; repeat with `high-precision` when changing
fixed-point conversion. Compile Redis restoration coverage with
`cargo test --locked --profile nextest -p nautilus-infrastructure --features redis --test redis_restoration --no-run`.
Real Redis/PostgreSQL service execution remains an integration activation gate.

For TLS or feature changes, build the affected rows and the feature union, then run the local
certificate/provider tests documented in [OrderHub TLS](docs/orderhub/tls.md). These tests use
local protocol listeners and fixed certificate fixtures; they do not validate real database
authentication, transactions or reconnect behavior. Keep normal database activation disabled
until its separate service gate passes. Retained benchmarks use native Criterion with external
plotting disabled; do not restore CodSpeed, Iai or their C backend paths.

Remote GitHub activity and releases require the project owner's authorization.

The inherited English-only editorial hook is manual. Chinese OrderHub contracts and Unicode
regression fixtures are valid local inputs; language style is not a business or security gate.
