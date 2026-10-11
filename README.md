# NautilusTrader

This repository is an **independently maintained local derivative** of
[NautilusTrader](https://github.com/nautechsystems/nautilus_trader), adapted for an internal
OrderHub. The name and existing `nautilus-*` crate and API names remain for source compatibility.
This checkout is not an official Nautech Systems release, and upstream support, CI badges,
package indexes, and release channels do not describe it.

The source retains its original copyright notices and LGPL-3.0-only license. See [LICENSE](LICENSE)
and [TRADEMARK.md](TRADEMARK.md) for the applicable license and upstream trademark information.
Git history and inherited source files retain the project's provenance.

## Local scope

OrderHub adds a gateway, a node, and a gRPC contract around the Nautilus trading core:

- `crates/orderhub-gateway`: request admission bridge and local business journal.
- `crates/orderhub-node`: assembly and service binaries.
- `crates/orderhub-proto`: service contract and generated Rust types.
- `crates/orderhub-tools`: schema and repository checks, plus an optional offline catalog converter.

The retained Rust core includes Cache, execution, risk, portfolio, and OMS market inputs.
OrderHub owns lifecycle, journaling, checkpoints, and recovery. Redis and PostgreSQL integrations
remain as optional Rust backends; real service validation is required before activation.
The node currently rejects database activation flags. The research/backtest framework, general catalog, generic node, Python bindings,
and binary bus codecs are retired; source history is retained in the
[P7 retirement record](docs/orderhub/framework-retirement.md). Historical Parquet conversion is
isolated behind `orderhub-tools/legacy-catalog`. The OrderHub design documents in
[docs/orderhub](docs/orderhub) describe goals
and contracts; they should not be read as proof that every proposed behavior is implemented.

This software can execute live trades involving real capital. Validate risk controls, venue
configuration, persistence, recovery, and reconciliation for the intended environment before
connecting it to a live account.

The base node/client allow explicit loopback plaintext only. `grpc-tls` requires configured
certificate/key and explicit client trust/domain, with no plaintext fallback. `redis`,
`postgres`, and `postgres-tls` retain their build paths; the union shares one TLS provider.
See [TLS configuration and boundaries](docs/orderhub/tls.md).

P0–P8 passed their scoped Windows x64 local acceptance on 2026-10-11. See the
[P8 acceptance record](audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)
for evidence and deferred service, deployment and trading activation gates.

The subsequent `local-usd-v2` business rule revision removes the ±10% price band and quote
age/continuity admission gates. Valuation uses the last legal quote matching the instrument;
account/status freshness, cash/ownership limits, FIX, persistence and recovery gates still apply.
See the [current rule amendment](orderhub-project-simplification.md) and
[input contract](docs/orderhub/market-and-account-input.md). Original stage records retain their
original rule and evidence scope.
The [rule plan](docs/orderhub/quote-policy-v2-plan.md) and
[acceptance summary](docs/orderhub/quote-policy-v2-acceptance.md) record the subsequent change.

## Build and test from this checkout

Use the stable Rust toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml), including
its rustfmt and Clippy components. From the repository root:

```bash
cargo fmt --all -- --check
cargo check --locked --profile nextest --workspace --all-targets
cargo clippy --locked --profile nextest -p orderhub-gateway -p orderhub-node -p orderhub-proto -p orderhub-tools --all-targets -- -D warnings
cargo test --locked --profile nextest -p orderhub-gateway -p orderhub-node -p orderhub-proto -p orderhub-tools --lib --bins
```

These checks compile and test local code without invoking the separate local exchange executable.
The integration tests for Redis, PostgreSQL, market
data, and venue connectivity may require additional local services or fixtures; see their
respective test documentation before running them. The CI workflow does not start a separate
`rust-trader` exchange process.

`nautilus-testkit` retains the in-memory Cache double and event collection helpers.
Tests use checked-in local fixtures and do not download datasets.

The supported development and schema tools require no Python/PyO3 environment. See
[CONTRIBUTING.md](CONTRIBUTING.md) for the local change and validation process.

## Documentation and project routing

- [OrderHub design and status](docs/orderhub)
- [Developer guide](docs/developer_guide) (inherited technical reference; upstream release and
  contribution procedures do not govern this checkout)
- [Security reporting](SECURITY.md)
- [Local roadmap](ROADMAP.md)
- [AI policy](AI_POLICY.md)

Local issues, security reports, deployment decisions, and release decisions belong to the owner
of this checkout. Do not send them to upstream NautilusTrader services or maintainers. The
historical [CLA.md](CLA.md) remains as source history and does not gate local contributions.
The `kmind-git/nautilus_trader` Git remote is a personal fork; these local changes have not been
published as a separate GitHub project.
