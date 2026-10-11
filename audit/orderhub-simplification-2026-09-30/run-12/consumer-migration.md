# P3 deletion boundaries

The owner explicitly requested implementation of P3 in `orderhub-project-simplification.md`,
including retiring the plugin, Python sources/bindings and general examples after migration.
The source snapshot `p3-source-before.zip` contains 2,742 files; `backup.py` verified every
archived byte against `p3-source-before-hashes.json` before source edits began.

## Dispersed bindings

The source-preserving Rust parser removed Python-only items and `cfg_attr` annotations from
the retained sources. Cargo manifests no longer expose Python/extension-module features or
PyO3 dependencies. `rg` outside the binding/example/plugin trees finds no `mod python`,
`::python`, `pyo3::`, `pyo3_stub_gen` or Python cfg consumers. The standalone top-level binding
crate was already retired in P0. These directories contain only the retired binding
implementation; domain implementations, DB/FIX and Rust business tests remain outside them.

## Plugin

The SDK has no retained workspace consumers. The live plugin lifecycle was an `Ok(())` stub;
its module, field and calls were removed. Legacy nonempty plugin configuration is explicitly
rejected, avoiding silent acceptance. The SDK and its host stub have no remaining runtime
references; stale descriptions are updated separately.

## Examples

Cargo `autoexamples` is disabled and explicit general example targets have been removed.
The active EMA/Grid/Imbalance research integration tests retain their original assertions.
Their three fixtures were copied into `backtest/tests/integration/workload_support`, and the
imports/dev dependency now use this test-only module and `nautilus-indicators`.
`testkit/datasets` and lifecycle testers remain explicit research consumers until P7.
General examples have no OrderHub runtime role. No admission, risk, FIX, DB or recovery test
is included in the example deletion boundary.

Broad simultaneous deletion was rejected by automatic approval review. Subsequent deletion
requests are limited to these separately audited directories, with this consumer record and
the recoverable source snapshot.

## Explicit owner approval and verification boundary

The owner approved the following recommendation: remove `examples/`,
`crates/backtest/examples/`, `crates/common/examples/`, and `crates/trading/src/examples/`;
retain all four named backtest integration test files; migrate and verify the necessary Arrow
generator before removing `crates/persistence/examples/`.

`example-deletion-preflight.json` records every file in these five trees: 112/4/5/32/1 files.
Every SHA-256 matches the recoverable source snapshot. This authorizes a total of 154 source
files for scoped retirement, including the old generator only after successful verification.

`workload-migration-parity.json` compares the four integration files and three relocated fixture
test files. All 63 test function names and 147 complete assertion expressions are unchanged.
The ITCH module retains its original `high-precision` condition; its large Parquet dataset is
not installed on this machine, so source retention and compilation do not count as execution.

The generator now has an explicit Rust binary entry at
`crates/persistence/bin/generate_arrow_fixtures.rs`, behind `fixture-tools`. `--output-dir`
allows verification without replacing checked-in fixtures. Its existing Zstd/Parquet research
dependencies remain a P7/C4 consumer: migration of this entry is not final no-C acceptance.
