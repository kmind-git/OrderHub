# Rust

This page defines NautilusTrader conventions for Rust source, Cargo manifests, and tests.

NautilusTrader uses Rust for its mission-critical core because the language combines a strong type
system, an ownership model, and predictable performance. Safe Rust prevents data races and many
memory errors at compile time. `unsafe` code must make explicit the invariants that the compiler
cannot check.

`rustfmt` and the workspace lints own general Rust style; the rules below supplement them.

## Sources of truth

| Concern                    | Source                                             |
| -------------------------- | -------------------------------------------------- |
| Formatting and imports.    | `rustfmt.toml`.                                    |
| Workspace lints.           | `Cargo.toml` and `clippy.toml`.                    |
| Cargo layout.              | `.pre-commit-hooks/check_cargo_conventions.sh`.    |
| Rust layout.               | `.pre-commit-hooks/check_formatting_rs.sh`.        |
| Nautilus type conventions. | `.pre-commit-hooks/check_nautilus_conventions.sh`. |
| Async boundaries.          | `.pre-commit-hooks/check_tokio_usage.sh`.          |
| DST boundaries.            | `.pre-commit-hooks/check_dst_conventions.sh`.      |
| Anyhow.                    | `.pre-commit-hooks/check_anyhow_usage.sh`.         |
| Error names.               | `.pre-commit-hooks/check_error_conventions.sh`.    |
| Logging.                   | `.pre-commit-hooks/check_logging_conventions.sh`.  |
| Rustdoc contracts.         | `.pre-commit-hooks/check_docs_conventions.sh`.     |
| Test style.                | `.pre-commit-hooks/check_testing_conventions.sh`.  |
| Workspace test selection.  | `scripts/ci/check-workspace-test-coverage.sh`.     |

Match nearby code when the tools do not settle a choice. Change generator inputs and regenerate
outputs instead of editing generated files directly.

## Module layout

Arrange each hand-written module so its primary behavior appears before its supporting
implementation details.

### Module roots and declarations

Preserve the surrounding module-root style when adding child modules. In each contiguous
declaration block in a `mod.rs` file, order out-of-line module declarations by these sections:

1. `#[macro_use]` modules.
1. Public modules.
1. Restricted modules such as `pub(crate)`.
1. Non-test `#[cfg(...)]`-gated modules.
1. Private modules.
1. Test-only modules.

Alphabetize declarations within each section and leave one blank line between sections. The
formatting hook enforces this order in `mod.rs` files.

Use the narrowest visibility that serves the caller. The workspace denies unreachable `pub` items.

### Imports

Keep imports at the top of the file or module. Use a local import only when its narrow scope
materially improves clarity.

### Constants and global state

Group module-wide constants and global state near the top of the module. Put `const` items before
`static` and `thread_local!` declarations. Keep a narrowly used constant or static next to its
consumers instead.

### Primary types and implementations

Keep the primary type and its inherent implementation near the top of a module.

### Enums

Place enums by role rather than collecting them in one module-wide block:

- Keep a primary or public enum with the module's primary types.
- Keep a small state enum next to the global whose state it represents.
- Place a private supporting enum below its first caller.

### Supporting definitions

Place private functions and types below their callers. In adapters, place private route types,
decision enums, and parsing functions below the main client implementations.

### Box-style banner comments

Do not add box-style banner or separator comments to divide module contents. Use modules and
implementation blocks to express structure. The standard copyright and license header is the
exception.

## Cargo manifests

### Dependencies and sections

- Use workspace inheritance for shared dependencies, for example
  `serde = { workspace = true }`. Pin a version in a crate only when the dependency is not
  workspace-managed.
- Separate dependency groups with a blank line and alphabetize each group. Manifests normally group
  internal `nautilus-*` crates, required external crates, and optional external crates, but preserve
  a manifest's meaningful local groups.
- Keep optional crates in their own blank-line group, except when every crate in the group is a
  `nautilus-*` crate.
- Keep the standard section order: package, lints, library, features, `cargo-machete` metadata,
  docs.rs metadata, dependencies, development dependencies, build dependencies, benches, binaries,
  examples, and tests.
- Add `[lints] workspace = true` to every workspace crate with a library or binary target.
- Keep adapter dependencies in the `# Adapter dependencies` section of the workspace
  `Cargo.toml`. Core crates must not depend on entries from that section.
- Keep related dependency families compatible. The Cargo convention hook checks the known
  constraints, including `capnp` with `capnpc`, `arrow` with `parquet`, and
  `dydx-proto` with `prost` and `tonic`.
- List only declared dependencies under `[package.metadata.cargo-machete] ignored`.
- Remove a root `[workspace.dependencies]` entry when no crate uses it. Cargo tools kept only for CI
  are exempt from this check.
- Remove a root `[workspace.package]` field when no crate inherits it.
- List each `[workspace] members` entry as a literal path. The convention hook resolves
  member manifests directly and does not expand Cargo glob members.
- Keep `libfuzzer-sys` outside the retained workspace. Optional fuzzing requires a separately
  configured experiment and cannot replace stable validation.

### Package fields

Crate `[package]` sections use this canonical prefix. Cargo infers `README.md` next to the
manifest, so omit `readme`. The other fields shown are required.

```toml
[package]
name = "nautilus-example"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
description = "Example crate for NautilusTrader"
categories.workspace = true
keywords.workspace = true
documentation.workspace = true
repository.workspace = true
homepage.workspace = true
```

Place the optional `publish`, `build`, and `include` fields after `homepage.workspace`.

### Features

- Preserve the crate's existing default feature contract. Most core crates have empty defaults,
  while many adapter crates enable `high-precision` by default.
- Python/PyO3 and dynamic-plugin features are retired in this local derivative.
- Propagate `high-precision` to dependent Nautilus crates that store or construct fixed-point
  domain values.
- Document each public non-default feature once, in alphabetical order, under `## Feature flags` in the
  crate README and `# Feature Flags` in the crate-level library docs.

### Targets

- Use snake_case filenames for `bin/` sources and kebab-case executable names, for example
  `path = "bin/ws_data.rs"` and `name = "hyperliquid-ws-data"`.
- Set `doc = false` on binary and example targets. Maintenance binaries retain unit tests; other binary targets may set `test = false`.
- Order library crate types as `rlib`, `staticlib`, then `cdylib` when a crate produces more than
  one type.

## File header requirements

Copy the standard copyright and license header from a neighboring hand-written Rust file. Generated
files retain their generator header instead. The copyright hook checks the year.

Change a generator input and rerun the generator instead of editing generated Rust, C headers,
or other generated artifacts.

## Formatting and attributes

The pinned stable `rustfmt` formats import lines but does not enforce import grouping. Keep
standard library, external crate, and local imports in separate groups, alphabetized within each
group. Run `cargo fmt --all` after editing.

Leave one blank line:

- Between functions, including tests.
- Above each `///` or `//!` doc comment.
- Above standalone `if`, `match`, `for`, `while`, and `loop` expressions.
- Above task spawn calls, including `spawn_local` and `spawn_blocking`.
- Before a `let` that contains a multiline braced expression, unless it starts a block.
- Before a multiline struct literal or a direct qualified `Type::new(...)` call, unless it starts a block.
- After a statement that contains a multiline braced expression, before the next statement in the same block.

The control-flow and spawn rules do not apply when the expression starts a block, continues the
previous operation, or has an attached comment or attribute. Wrapped calls and method chains alone
do not need a separator. Construction spacing also applies to assignments, explicit returns, and
calls followed by `?` or `.await`. Construction must span multiple lines after rustfmt layout;
one-line initializers stay together.

Keep comments and attributes attached to their statement when inserting a separator. Skip macro
bodies and `rustfmt::skip` regions. Apply the multiline statement rules around changed code; leave
unrelated code alone. Add only blank lines that `rustfmt` preserves.

The formatting hook:

- Checks existing control-flow, spawn, and module-ordering rules at changed boundaries.
- Compares staged and unstaged changes against `HEAD`, or against the merge base with `CHANGED_BASE_SHA` when set.
- Falls back to checking all tracked Rust files when a CI base is unavailable.
- Reads complete changed files for context and includes lines used by exemption checks when selecting diagnostics.
- Does not modify files.

These remain review conventions:

- Multiline statement spacing.
- Construction spacing.
- `spawn_local` and `spawn_blocking` spacing.

Keep changed code readable for a human reader, and add further blank lines when those separators
still leave a dense block hard to follow.

Use inline format arguments for existing variables:

```rust
anyhow::bail!("Failed to subtract {n} months from {datetime}");
```

Add `#[must_use]` to constructors, accessors, pure conversions, and consuming `with_*` methods when
discarding the returned value is almost certainly a mistake. A return type such as `Result` already
carries its own `must_use` annotation.

When suppressing `missing_panics_doc` or `missing_errors_doc`, include a reason that explains why
the lint does not apply:

```rust
#[allow(
    clippy::missing_errors_doc,
    reason = "result type is retained for API compatibility but no error is returned",
)]
```

Use `try_update` instead of the deprecated `fetch_update` on standard atomics. The pre-commit hook
rejects `fetch_update`.

## Type qualification

| Item                          | Convention                                                                  |
| ----------------------------- | --------------------------------------------------------------------------- |
| `anyhow`.                     | Import only `anyhow::Context`; fully qualify macros and `Result`.           |
| Nautilus domain types.        | Import the type, then use its short name.                                   |
| Tokio time, sync, and tasks.  | Fully qualify the path; use `std::time::Duration`.                          |
| `Debug` and `Display`.        | Import the trait.                                                           |
| `Formatter` and `fmt::Result` | Use `std::fmt::Formatter` and `std::fmt::Result` at the implementation.     |
| Nautilus macros.              | Import `nautilus_actor!` or `nautilus_strategy!`, then call it unqualified. |

Use `debug_struct(stringify!(TypeName))` in manual `Debug` implementations.

Use `// nautilus-import-ok` only where a macro, generated path, or conditional import requires a
fully qualified Nautilus type. Put the marker on the affected line or directly above the narrow
block.

## Error handling and contracts

### Design by contract

Use the narrowest mechanism that expresses the contract:

| Situation                                          | Mechanism                                    |
| -------------------------------------------------- | -------------------------------------------- |
| Compile-time state or ownership rule.              | Types, lifetimes, newtypes, and visibility.  |
| Public input precondition.                         | `check_*` from `nautilus_core::correctness`. |
| Validated value construction.                      | `new_checked()` with a `new()` wrapper.      |
| Recoverable parse, I/O, or network failure.        | `Result<T, E>`.                              |
| Internal invariant safe to omit in release builds. | `debug_assert!`, covered by a targeted test. |
| Soundness or always-on invariant.                  | `assert!` or a checked error path.           |

Place an assertion where the code first relies on the invariant. Never use `debug_assert!` for
public input validation or a soundness condition because release builds remove it.

Prefix debug assertion messages with `Invariant:` and state the positive rule. The shared
`Condition failed: ...` prefix marks a caller input violation; `Invariant: ...` marks an internal
contract bug.

### Error boundaries

Choose the error type at the API boundary:

| Boundary                              | Return type                         |
| ------------------------------------- | ----------------------------------- |
| Reusable library or domain API.       | A typed `Result<T, E>`.             |
| Application or adapter orchestration. | `anyhow::Result<T>`.                |
| Public input validation.              | `CorrectnessResult<T>` when suited. |

- Define a typed error with `thiserror` when callers inspect or recover from the failure.
- Bind error patterns and closures as `e`, not `err` or `error`.
- Use `anyhow::bail!` for an early return from an `anyhow::Result` function. Use
  `anyhow::anyhow!` when an error value is required, such as inside `ok_or_else`.
- Start `.context()` messages with lowercase text so chained errors read naturally. Preserve the
  capitalization of a leading proper noun or acronym.
- Do not use `", got"` in an error or assertion. Use `", was"`, `", received"`, or `", found"`
  according to the context.

```rust
parse_timestamp(value).context("failed to parse timestamp")?;
connect().context("BitMEX websocket did not become active")?;
```

### Panic policy

Panics are valid for internal invariant violations that callers cannot reasonably recover from.
Keep an API infallible when returning `Result` would only require callers to handle a programming
defect. Use an always-on assertion for an invariant that protects soundness or prevents unsafe code
from continuing with invalid state.

Return `Result` at boundaries that accept untrusted input or can fail because of configuration,
I/O, external data, task cancellation, or resource lifecycle. These are operational failures even
when a specific call site expects them never to occur. For validated values, follow the
[`new()` and `new_checked()` constructor pattern](#constructor-patterns): expose a fallible path for
untrusted values and use the convenience wrapper where its caller contract permits a panic.
Document reachable panics in public Rust APIs, and make each panic message name the violated
invariant.

Treat findings for these lints from `make clippy-strict-audit` as review prompts:

- `clippy::panic`
- `clippy::unwrap_used`
- `clippy::expect_used`

The audit also reports `clippy::panic_in_result_fn`, which the required workspace Clippy gate
already enforces. Remove a panic when the failure is recoverable; retain a justified invariant
panic, with a scoped lint reason when needed. The audit uses forced warnings, so its totals include
deliberate sites with local lint allowances and diagnostics from macro expansions.

### Failure contract examples

APIs with a documented panic contract use panics for:

- Programmer errors (logic bugs, incorrect API usage).
- Data that violates fundamental invariants (negative timestamps, NaN prices).
- Arithmetic that would silently produce incorrect results.

APIs return `Result` or `Option` when callers, including downstream crates, can handle a failure or
absence, including:

- Expected runtime failures (network errors, file I/O).
- Business logic validation (order constraints, risk limits).
- User input validation.

The API determines how an invalid operation fails:

```rust
let later = timestamp + duration_ns; // Panics on overflow.

let price = Price::new_checked(f64::NAN, precision); // Returns Err.

let later = timestamp.checked_add(duration_ns); // Returns None on overflow.
```

This policy is implemented throughout the core types (`UnixNanos`, `Price`, `Quantity`, etc.)
and helps NautilusTrader maintain strong data correctness for production trading.

The repository release profile sets `panic = "abort"`, so a panic terminates the process for a
supervisor or orchestration system to handle. Downstream Rust binaries control their own release
profile.

## Logging

- Fully qualify log macros, for example `log::debug!` and `log::info!`.
- Start messages with a capitalized word and omit terminal periods.
- Use `// log-period-ok` on the call or within three lines above it when a terminal period is part of
  the logged value rather than sentence punctuation.
- Keep connection and client lifecycle, reconnection, reconciliation, and mass-status summaries at
  `INFO`.
- Keep subscription detail, per-order confirmations, instrument counts, authentication, and
  WebSocket internals at `DEBUG`.
- Log a warning or error when an unexpected, user-actionable, or data-loss condition is handled
  locally. Do not log an error that continues to propagate through `?` or `anyhow::bail!`.
- Leave a blank line above a log call unless it is the first line of the function.
- Do not write to stdout or stderr or terminate the process from production library code. Binaries,
  examples, benches, tests, adapters, the CLI, and testkit may control their process when that is
  part of their role.

## Async code

Synchronous core crates do not take Tokio as a regular dependency. The `common` crate keeps Tokio
optional. The Tokio convention hook owns the exact crate list.

Retained client code uses the shared runtime because calls may arrive without
a thread-local Tokio context:

```rust
use nautilus_common::live::get_runtime;

get_runtime().spawn(async move {
    run_client().await;
});
```

- Import `get_runtime` through `nautilus_common::live`, not `live::runtime`.
- Use `get_runtime().block_on()` when synchronous adapter code must call an async function.
- Use the runtime supplied by `#[tokio::test]` in tests; `tokio::spawn()` is valid there.
- Put `// tokio-import-ok` on an import or spawn line only when the fully qualified form or shared
  runtime cannot serve that site.
- Install a custom runtime before `LiveNode::build()` or any adapter use. Build a multi-threaded
  runtime with all drivers enabled.
- Install any custom runtime before building consumers; do not swap it during execution.

Code on the deterministic simulation path follows the
[DST determinism contract](../concepts/dst.md#determinism-contract). Route clocks, random values,
task spawning, and network access through the project seams. Use `biased;` in `tokio::select!`
blocks on that path.

## Runtime ownership and access

### Cache order access

`Cache` hides its `SharedCell` order storage behind lifetime-scoped accessors. Use `order_ref()` or
`try_order_ref()` for scoped reads, `order_mut()` for an exclusive write borrow, and `order_owned()`
or `try_order_owned()` when a snapshot must cross a boundary. The `try_*` forms return
`OrderLookupError` when the order is absent.

`order_mut()` requires `&mut Cache`, so adapter-facing `CacheView` code cannot mutate orders. Drop an
`OrderRef` or `OrderRefMut` before dispatching events or taking a borrow that can re-enter the same
order, then look up the order again for post-event state.

### Runtime invariants

The actor registry, component registry, and message bus use `thread_local!` storage. Objects
registered on one thread are not visible from another. `LiveNodeHandle` is the intended
cross-thread control surface.

Keep an `ActorRef` within one synchronous scope. Do not store it in a struct, hold it across
`.await`, or send it to another thread. Capture the actor ID in long-lived callbacks and look up the
actor each time the callback runs.

The component registry rejects aliased mutable access with a scoped borrow guard. Do not make
component lifecycle operations re-entrant.

## Construction and conversion

### Constructor patterns

Validated value types pair a fallible `new_checked()` with a convenience `new()`:

```rust
pub fn new_checked<T: AsRef<str>>(value: T) -> CorrectnessResult<Self> {
    let value = value.as_ref();
    check_valid_string_ascii(value, stringify!(value))?;
    Ok(Self(Ustr::from(value)))
}

pub fn new<T: AsRef<str>>(value: T) -> Self {
    Self::new_checked(value).expect_display(FAILED)
}
```

Use the shared `FAILED` constant with `CorrectnessResultExt::expect_display` so panic messages use
the standard `Condition failed: ...` prefix. Document the error on `new_checked()` and the panic on
`new()`.

For types with long constructors dominated by optional fields, use a `bon` builder instead of
public positional `new()` and `new_checked()` methods. Put `#[bon::bon]` on the inherent
implementation and make the builder's finish method delegate to a private validation function or
perform the validation directly. Keep one validation and defaulting path.

```rust
#[builder(start_fn = builder, finish_fn = build)]
pub fn build_checked(/* same inputs as new_checked */) -> CorrectnessResult<Self> {
    Self::new_checked(/* forward inputs */)
}
```

Required fields remain required in `bon` typestate. Optional fields remain omittable, and `build()`
returns the same `CorrectnessResult` as the internal validation path.

### Conversion patterns

- Implement `From` only for infallible, complete conversions.
- Implement `TryFrom` when the source can contain an invalid or unrepresentable value.
- Implement `FromStr` for string parsing.
- Treat generic `From<T: AsRef<str>>` implementations that panic as compatibility surfaces. Do not
  copy that pattern into new APIs.
- Keep venue wire enums distinct from Nautilus domain enums. Use idiomatic Rust variant names,
  express the wire spelling with `serde` or `strum` attributes, and convert explicitly at the
  boundary.
- Deserialize adapter payloads into wire models before constructing domain objects. Keep parsing
  and validation in conversion functions instead of embedding venue wire details in domain types.

### Domain numeric types

Preserve discrete financial values as decimals from ingestion:

| Value                                    | Type and construction                                    |
| ---------------------------------------- | -------------------------------------------------------- |
| Price or quantity.                       | `Price::from_decimal_dp` or `Quantity::from_decimal_dp`. |
| Money, fee, margin, or balance.          | `Decimal`, then `Money::from_decimal` or `Money::zero`.  |
| Continuous signal ratio or timing curve. | `f64` when decimal precision has no domain meaning.      |

Do not route wire values through `f64` constructors. In tests, compare `.as_decimal()` with
`dec!(value)`.

## Identifier storage

### How string interning works

String interning stores one shared copy of each distinct string in a central cache. Repeated values
refer to the same cached bytes instead of allocating another copy. Small handles make the values
cheap to copy and compare, while a cached hash avoids reading the full string again during hashing.

NautilusTrader uses `Ustr` for its interned identifier components. Each `Ustr` is a pointer-sized
`Copy` handle with a precomputed hash and stable direct string access. Composite types such as
`InstrumentId` preserve the same cheap copy semantics by storing these handles.

### Reclamation boundary

The string cache retains every unique value for the process lifetime. This retention keeps copied
handles and returned string slices valid without reference counting, access guards, or explicit
lifetime parameters on identifier types. Process teardown is the normal reclamation boundary.

These guarantees rule out safe reclamation of individual entries. Rust can copy a `Copy` value
without executing code, so an atomic reference count cannot observe every copy. Designs that add
reclamation change the identifier contract:

- Reference counting requires `Clone` and `Drop`, which removes `Copy` from identifiers and types
  that contain them.
- Borrowed or epoch-protected storage requires lifetimes or access guards at string access points.
- Generational handles permit reclamation but make lookup fallible and invalidate stale handles.
- A global cache reset is safe only at a proven quiescent point after all handles, references, and
  foreign pointers have been destroyed and no task or thread can retain one.

### Storage boundaries

Interning is best suited to identifiers drawn from a bounded process-scoped universe and values that
repeat enough to benefit from deduplication. Identifiers whose distinct values can grow with every
order, trade, or message increase the cache for the process lifetime.

Fixed-capacity inline storage retains `Copy` when the external protocol supplies a suitable maximum.
`TradeId`, for example, uses a 36-character `StackStr`. Owned or reference-counted storage provides
dynamic capacity when reclamation matters more than `Copy`.

The domain model also contains compatibility exceptions. `ClientOrderId`, `VenueOrderId`,
`PositionId`, and `OrderListId` remain `Ustr`-backed and therefore retain every distinct value.
Identifier storage participates in the supported by-value C ABI, so a broader redesign depends on
conversion-based bindings replacing raw layout sharing.

The storage boundary includes an up-front estimate of every unique value and all intermediate
strings interned during parsing. The cache is shared by every `Ustr` use in the process, so its
memory cost is the aggregate set rather than a separate budget for each identifier type.

### Polymarket scale example

A Polymarket instrument symbol combines a 66-byte condition ID with a 77- or 78-byte token ID, for
a 144- or 145-byte interned symbol. With the 64-bit `ustr` 1.1.0 layout, 600,000 unique
`InstrumentId` values require roughly 150 MiB for the retained identifier values, cache lookup
table, and reserved string storage.

The Polymarket parsing path also interns each raw token ID and each condition ID. For 600,000
instruments from about 300,000 markets, these entries raise the estimate to roughly 300 MiB before
instrument objects, descriptions, maps, and other metadata. The estimate assumes unique instrument
and token IDs and includes capacity reserved by the cache's geometric allocator, so it is not an
exact resident-set measurement.

NautilusTrader accepts this bounded cost to preserve `Copy`, stable direct access, and global
deduplication across the instrument universe. Unbounded streams of unique external IDs remain
outside this storage model.

## Collections

Choose a hash collection by iteration semantics and trust boundary:

| Requirement                                    | Collection                |
| ---------------------------------------------- | ------------------------- |
| Observable insertion-order iteration.          | `IndexMap` or `IndexSet`. |
| Hot lookup with no observable iteration order. | `AHashMap` or `AHashSet`. |
| Keys chosen by an untrusted third party.       | `HashMap` or `HashSet`.   |
| External API requires a standard collection.   | `HashMap` or `HashSet`.   |

Default `AHashMap` and `AHashSet` instances randomize their hasher state, so their iteration order is
not stable. When iteration order affects emitted events, commands and traffic sent to a venue,
persisted output, or random number consumption, make the order deterministic. An iteration whose
order no caller observes does not need stabilization.

Use `IndexMap` or `IndexSet` when insertion order is both deterministic and the required sequence.
Otherwise, keep the hash collection and sort at the point of use. Prefer sorting when ordered
traversal is infrequent or order-preserving removal is hot. `shift_remove` preserves relative order
in O(n) average time; `swap_remove` runs in O(1) average time but can move the last entry.

`AHash` is not cryptographically secure. Use `HashMap` or `HashSet` where untrusted keys make
hash-flooding resistance part of the security boundary.

## Documentation

### Coverage and tone

- Add doc comments to public items. Document private behavior with a normal comment only when
  non-obvious context prevents a likely misreading.
- Add module documentation to public modules and modules with a non-obvious contract. Do not add
  boilerplate to a private leaf module.
- Use the indicative mood: "Returns the account ID", not "Return the account ID".
- End public field and enum variant documentation with a period.
- Match documentation density to neighboring items. Do not add filler comments to make a bare block
  look uniform.
- Put important context for private fields in the type-level documentation instead of documenting
  each field.

Document public functions when their contract is not fully clear from the type and name. Cover
fallible conditions, panic conditions, safety obligations, and non-obvious input semantics.

### Rustdoc sections

Use Title Case for Rustdoc section headings:

- `# Examples`
- `# Errors`
- `# Panics`
- `# Safety`
- `# Notes`
- `# Thread Safety`
- `# Feature Flags`

Use one sentence for a single error or panic condition:

```rust
/// # Errors
///
/// Returns an error if the currency conversion fails.
```

Use bullets with terminating periods for multiple conditions:

```rust
/// # Errors
///
/// Returns an error if:
/// - The market price cannot be found.
/// - The conversion rate calculation fails.
```

Use `# Errors` only on a function that returns `Result`, `PyResult`, or `Option`. Use `# Panics` only
when the function can panic, and remove the section instead of saying that the function does not
panic.

The documentation hook recognizes direct panic sites in a `Result`-returning function. Put
`// panics-doc-ok` immediately above the doc block when a called function supplies the documented
panic. Use `// errors-doc-ok` in the same position only for a special error contract that the
signature check cannot recognize.

### Doc examples

`make cargo-test-doc` compiles and runs doc examples. Annotate every fence:

| Fence          | Behavior                  | Use for                                                |
| -------------- | ------------------------- | ------------------------------------------------------ |
| `rust`         | Compiled and run.         | Self-contained examples with no external dependencies. |
| `rust,no_run`  | Compiled, not run.        | Examples needing a catalog, network, or venue.         |
| `ignore`       | Neither compiled nor run. | Pseudocode; state why it cannot compile.               |
| `compile_fail` | Must fail to compile.     | Demonstrating a rejected usage.                        |
| `text`         | Not code.                 | Directory trees, output, or diagrams.                  |
| `bash`, `json` | Not Rust.                 | Commands or payloads.                                  |

Prefer `no_run` to `ignore` when only the runtime dependency is unavailable. Prefix setup lines with
`#` when they must compile but would obscure the rendered example.

Put examples on public items. Rustdoc also collects fences from private item docs, but those examples
cannot import the item they document.

## Testing conventions

- Use `mod tests` for ordinary inline tests.
- Use `#[rstest]` instead of `#[test]`, including for a non-parameterized test.
- Use `#[tokio::test]` for a non-parameterized async test.
- Keep `#[cfg(test)]` on test modules and test-only files. Do not add test behavior or interfaces to
  production code.
- Store JSON fixtures under the crate's `test_data/` directory and load them with `include_str!`.
- Use distinct, non-default inputs and exact expected values. Assert every stable field.
- Group assertions after setup and actions unless the test checks stepwise state changes.
- Do not use Arrange, Act, Assert separator comments.

Every workspace member appears exactly once in `CORE_CRATES`, `ADAPTER_CRATES`, or `NO_TEST_CRATES`
in the `Makefile`. Add a crate to `NO_TEST_CRATES` only when it defines no Rust test target and
contains no `#[test]`, `#[rstest]`, or `#[test_case]` function.

Parametrize cases when the same behavior applies to several inputs:

```rust
#[rstest]
#[case("AUDUSD", false)]
#[case("CL.FUT", true)]
fn test_symbol_is_composite(#[case] input: &str, #[case] expected: bool) {
    assert_eq!(Symbol::new(input).is_composite(), expected);
}
```

### Test specs

Events with many constructor arguments use a fluent `bon` spec next to the event under
`events/order/spec/`. Gate the module with `#[cfg(any(test, feature = "test-support"))]` so downstream
tests can opt in without adding the spec to production builds.

- Derive `bon::Builder` with `finish_fn = into_spec`.
- Give required fields deterministic valid defaults. Leave optional fields as `Option<T>`.
- Default event IDs with `test_uuid()` from `crate::stubs`.
- Implement `build()` by forwarding through the production constructor.
- Return the event directly because spec defaults are valid by construction.
- Pin every default in one test in the spec module.

Override only the fields relevant to the caller:

```rust
let fill = OrderFilledSpec::builder()
    .last_qty(Quantity::from(50_000))
    .trade_id(TradeId::from("TRADE-1"))
    .build();
```

Under plain `cargo test`, call `reset_test_uuid_rng()` before a test that compares UUID sequences.
`cargo nextest` starts each test in a fresh process, so the sequence resets automatically.

### Property-based tests

Use `proptest` when an invariant spans an input class that examples cannot cover. Keep strategies
near the property suite and combine ranges with explicit edge cases.

Prefix property names with `prop_` and retain `#[rstest]` on tests inside `proptest!`. A large suite
may use a `property_tests` module or a dedicated file, but the repository also keeps focused
properties beside their unit tests.

## Unsafe Rust

Unsafe code must make its proof obligations reviewable:

- Give each unsafe function a `# Safety` section that states the caller's complete obligations.
- Put a `SAFETY:` comment directly above each unsafe operation and explain why its preconditions
  hold at that site.
- Wrap each unsafe operation in its own `unsafe { ... }` block. Workspace crates deny
  `unsafe_op_in_unsafe_fn`.
- Use always-on checks for null, alignment, provenance, and other soundness conditions.
- Add targeted tests for observable behavior around unsafe code. Tests support the proof but do not
  establish soundness.
- Treat an unsafe `Send` or `Sync` implementation as a proof over all reachable state, aliases,
  callbacks, generic parameters, safe methods, cloning, and destruction.

For raw vectors crossing FFI, follow the [FFI memory contract](ffi.md). Foreign code owns the
allocation after transfer and must call the matching `vec_drop_*` function exactly once.

## Other generated artifacts

### FFI bindings and precision

Only `nautilus-core` and `nautilus-model` expose an `ffi` feature. Check both crates directly when
changing their C ABI:

```bash
cargo check -q -p nautilus-core --features ffi
cargo check -q -p nautilus-model --features ffi,high-precision
```

The crate-local `cbindgen.toml` files define the header layout for native consumers. Do not add an
`ffi` feature or `src/ffi` module to another workspace crate.

Run `bash scripts/ci/check-cbindgen-abi.bash` to generate both headers, verify their public names
and compatibility enum values, and compile a C11 consumer against them. This retained manual
entry requires the pinned cbindgen version and a C compiler; it is not a Make target or a
required nightly Rust check.

### Serialization in this local derivative

The research Cap'n Proto/SBE generators and generic Arrow catalog are retired in P7.
Supported messages use native Serde JSON/MsgPack; gRPC schemas use Rust/prost.
Historical Parquet conversion is isolated in the optional `orderhub-tools/legacy-catalog` entry;
see [OrderHub tooling](../orderhub/tooling.md).
