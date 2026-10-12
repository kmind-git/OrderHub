# Testing

Our automated tests serve as executable specifications for the trading platform.
A healthy suite documents intended behavior, gives contributors confidence to refactor, and catches regressions before they reach production.
Tests also double as living examples that clarify complex flows and provide rapid CI feedback so issues surface early.

The suite covers these categories:

- Unit tests
- Integration tests
- Acceptance tests
- Performance tests
- Property-based tests
- Fuzzing
- Memory leak tests

## Testing policy

Tests and runtime contracts form one design system. The
[Design by contract](rust.md#design-by-contract) ladder pushes invariants into the type
system where possible; the testing ladder below escalates the remaining unknowns through
larger input spaces and richer execution models. Each layer extends coverage to inputs
or execution states the layer below cannot reach.

Not every module requires every technique. Use this section to decide which layers apply
before adding tests or `debug_assert!` statements.

### Mechanism ladder

Runtime contracts are covered in the [Rust guide](rust.md#design-by-contract): prefer the
type system first, then `check_*` from `nautilus_core::correctness` at API boundaries,
then `debug_assert!` for internal invariants, then `assert!` for soundness-critical or
always-on checks.

Test layers follow a parallel escalation. Start at the lowest layer that proves what
matters; climb only when the layer below stops detecting regressions or when the input
space grows beyond hand-picked cases.

| Layer                    | Trigger condition                                                               |
| ------------------------ | ------------------------------------------------------------------------------- |
| Unit test                | A single function or transition has a small, enumerable set of cases.           |
| Parametrized test        | The same shape repeats across discrete inputs (order side, status, instrument). |
| Property-based test      | An invariant must hold for a whole class of inputs the mind cannot enumerate.   |
| Integration test         | Multiple modules interact through a real (non-mocked) engine or runtime.        |
| Fuzz test                | Untrusted or adversarial bytes cross a parser, decoder, or wire-format handler. |
| Spec acceptance test     | Behavior depends on a live venue contract and its activation gate.              |
| Deterministic simulation | Correctness depends on task scheduling, timeouts, or wall-clock ordering.       |
| Formal verification      | A pure function has crisp invariants and a bounded input space worth a proof.   |

The formal verification rung is aspirational: no Kani or Prusti harness has landed in
the workspace. The row records the escalation condition for when a verifier is adopted,
not a current obligation.

### Projection rule

Module shape determines which layers pay off. Not every module warrants the full ladder.
Apply the rule at module granularity, not crate granularity: an adapter crate contains
pure parsers and I/O-bound client loops, and each row applies to a different part.

| Module shape                        | Layers that apply                             | Example                                |
| ----------------------------------- | --------------------------------------------- | -------------------------------------- |
| Pure function, crisp invariants     | Unit, parametrized, property, fuzz            | Reconciliation kernels, portfolio math |
| Pure function, no stated invariants | Unit, parametrized, property, fuzz            | Codecs, adapter parsers, formatters    |
| Stateful, synchronous               | Unit, parametrized, property over transitions | Cache, order book                      |
| Stateful, async                     | Unit, integration, deterministic simulation   | Live engine, execution manager         |
| I/O bound, venue contract           | Integration, spec acceptance, boundary fuzz   | Adapter client loops                   |

### When not to add coverage

- Add `debug_assert!` only where a test can reach it. Release builds strip the check, so
  an unexercised assertion has no signal. A targeted unit test counts as a harness; a
  proptest or fuzz harness amplifies the signal.
- Prefer a proptest over a hand-written edge-case test when the invariant spans a whole
  class of inputs. Targeted unit tests remain valid for known venue pathologies and as
  regression reproducers for shrunk counterexamples.
- Do not duplicate a live spec acceptance card as an integration test. Link to it instead.
- Do not pad coverage with tests that assert language or framework guarantees
  (`Option::is_some` after `Some(..)`, `Vec::len` after `push`).

### DST readiness

Deterministic simulation testing (DST) requires the runtime to be free of ambient
non-determinism. Before promoting a module to run under DST, verify the following:

- Time, task, runtime, and signal primitives route through `nautilus_common::live::dst`
  rather than `tokio` directly. Wall-clock reads go through the seam in
  `nautilus_core::time` rather than `SystemTime::now()` at call sites.
- State maps with ordering-dependent iteration use `IndexMap` or `IndexSet`, not the
  default hash collections.
- Every `tokio::select!` on a control-plane path sets `biased` so poll order is fixed.
- No calls to `Instant::now()`, `SystemTime::now()`, `tokio::signal::ctrl_c`,
  `std::thread::spawn`, or `tokio::task::spawn_blocking` escape the seam. Blocking-thread
  and OS-thread primitives break madsim determinism the same way an ambient clock read
  does.
- Replay-sensitive IDs (`trade_id`, `venue_order_id`) are pure functions of their inputs;
  see `crates/execution/src/reconciliation/ids.rs`. Ephemeral event UUIDs on other
  reconciliation paths do not need to be deterministic.

The `surface` probe in `crates/common/src/live/dst.rs` only pins the re-export shape;
it does not check that callers actually use the seam. Enforcement is by review. Run the
audit whenever a new async module enters the workspace or an existing module gains new
control-plane scheduling.

## Property-based testing

Property testing verifies that logic holds for *all* valid inputs, not just hand-picked examples.
We use [`proptest`](https://altsysrq.github.io/proptest-book/intro.html) in Rust to enforce invariants.

- **Use cases:** Core domain types (`Price`, `Quantity`, `UnixNanos`), accounting engines, matching engines, and state machines.
- **Example invariants:**
  - Round-trip serialization: `parse(to_string(value)) == value`
  - Inverse operations: `(A + B) - B == A`
  - Transitivity: `If A < B and B < C, then A < C`

## Running local Rust tests

Use the required commands in [CONTRIBUTING.md](../../CONTRIBUTING.md). Run the smallest relevant
tests while developing; broad tests are needed when a concrete shared dependency or behavior changes.
Do not require every research, performance or deployment test at each simplification phase.

```bash
cargo test --locked --profile nextest -p orderhub-tools --lib --bins
cargo test --locked --profile nextest -p orderhub-gateway -p orderhub-node --lib --bins
```

`make test` runs isolated OrderHub tests after stripping adapter environment variables.
`make cargo-test-core` adds retained framework library and binary tests. Both use `nextest` by
default as a Cargo profile, not as a requirement to install the nextest runner.
The profile retains assertions and overflow checks, with debug symbols disabled.

`orderhub-check test-network` flags literal external destinations and live opt-ins in test code.
It is a heuristic check, not an isolation sandbox. `workspace-tests` reconciles the Make crate
inventory. `retired-surfaces` rejects retired bindings/tools and checks retained FFI ownership.

## Integration activation

Compile retained PostgreSQL and Redis integration tests without starting services. Real service
tests run only against dedicated instances when activation is requested. Keep their tests and TLS
requirements; offline schema checks do not prove transaction or reconnection behavior.

The four retained backtest scenario tests use private workload fixtures. Large ITCH data is not
present on this computer, so execution of those dataset cases remains a P7 data gate. Their
compilation and the in-memory cases are separate evidence. Do not fetch fixtures implicitly or
claim missing-data execution passed.

Fuzzing or deterministic simulation applies where it detects a concrete boundary risk. Optional
nightly tools are experiments and never replace stable required gates. Final OMS business,
build/link/runtime and no-C validation is defined by
[the simplification plan](../../orderhub-project-simplification.md).

## Test style

Use `#[rstest]` for synchronous Rust cases and `#[tokio::test]` for async cases. Keep test-only
code under `#[cfg(test)]`. Preserve exact arithmetic, complete stable-field assertions and failure
behavior. Do not add production behavior just to make a test pass. Maintenance binary targets
keep their unit tests enabled. Python facade/import/ownership tests retire with their removed API.
