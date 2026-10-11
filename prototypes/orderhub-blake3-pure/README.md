# Pure BLAKE3 build probe

This standalone crate uses the repository-local `blake3` 1.8.7 patch with the `pure` feature.
Its fixed vectors were first produced by the unpatched published crate in
`../../audit/orderhub-simplification-2026-09-28/run-04/blake3-baseline` for empty input,
an OrderHub journal sample, and a 4097-byte boundary input. The test compares the patched
implementation with those values. The fresh target build in run-04 uses a failing compiler
sentinel to detect a C compiler call.

The patch does not change the hash algorithm. This small vector set does not establish
compatibility with every stored journal or checkpoint, nor performance equivalence.

```powershell
cargo fmt --manifest-path prototypes/orderhub-blake3-pure/Cargo.toml -- --check
cargo clippy --offline --locked --manifest-path prototypes/orderhub-blake3-pure/Cargo.toml --all-targets -- -D warnings
cargo test --offline --locked --manifest-path prototypes/orderhub-blake3-pure/Cargo.toml --lib
```
