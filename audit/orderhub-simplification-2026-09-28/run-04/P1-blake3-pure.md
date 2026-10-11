# P1 C3 partial: pure BLAKE3 path

Date: 2026-09-28. Platform: Windows x64, pinned Rust 1.98.1 stable. P1 remains open.

The published `blake3` 1.8.7 build script calls `c_compiler_support()` on x86 even when the
`pure` feature is selected. `patches/blake3-1.8.7` copies the published source, its license
files, and provenance hashes, and changes that one branch to skip the C compiler probe for
`pure`. The source algorithm is unchanged. The workspace now selects `pure` and the patch in
`Cargo.toml`; `Cargo.lock` records the local source.

The unpatched published crate produced these SHA-like BLAKE3 hexadecimal outputs in
`blake3-baseline` for empty bytes, `OrderHub persisted journal checksum fixture`, and 4097
bytes of `0xa5`, respectively:

```text
af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
b2db2cc1f8dd7060d3f05fe6501481399371c92cd90b9c06dd2d57c7fe99a4ae
6375aeecec916424bb516b46e3f37e89aead858cf69486bd8bee7157b980b63c
```

`prototypes/orderhub-blake3-pure` compared the patched `pure` implementation against all
three vectors: 1/1 test passed. A previously absent target directory and failing compiler
sentinel were used with `cargo test --offline --locked -vv --lib`; exit code 0 and full output
are `fresh-blake3-exit.txt` and `fresh-blake3-test.txt`. The build log selected Rust SSE2,
SSE4.1, and AVX2 cfgs and no FFI cfg. The `cc` Rust build dependency remains declared by
the upstream crate, but the sentinel was not invoked for this selected path.

`cargo fmt --manifest-path prototypes/orderhub-blake3-pure/Cargo.toml -- --check` and
`cargo clippy --offline --locked --manifest-path prototypes/orderhub-blake3-pure/Cargo.toml
--all-targets -- -D warnings` exited 0. The root workspace `cargo fmt --all -- --check`
exited 0. The event-store suite after root wiring passed 13 hash tests, including two
known vectors, and one snapshot-content-hash test. After disk recovery,
`cargo test --offline --locked -p nautilus-persistence --lib
stage_restored_metadata_hashes_canonical_schema_metadata` also passed its one selected test;
the output and exit code are `persistence-hash-test.txt` and `persistence-hash-exit.txt`.
These are bounded compatibility checks; stored historical journals/checkpoints and
performance have not been measured.

After this root manifest change, the OrderHub gate commands from `CONTRIBUTING.md` also
passed with `--offline --locked`: `cargo check` and `cargo clippy -- -D warnings` for
`orderhub-gateway`, `orderhub-node`, and `orderhub-proto` with `--all-targets`, plus
`cargo test --lib --bins` for those crates (six gateway tests passed; the other selected
library/binary targets had no unit tests). `cargo clippy -p nautilus-event-store --lib
-- -D warnings` passed as well. These checks do not substitute for the full P1 matrix.

The first patched fresh test rejected `--locked` because the prototype lockfile did not yet
exist. `cargo generate-lockfile --offline` created it, and the fresh test then passed. This
initial rejection was not a compiler or hash mismatch.

The first workspace persistence hash test did not reach its assertion: `rustc-LLVM` reported
`no space on device` while linking the large test target. The drive had approximately 9.8 MB
free at that point. The fresh prototype target directories created in this run and the
standalone prototype target directories were removed after verifying that their resolved paths
were under this repository; source, lockfiles, and captured text logs were retained. Free space
then measured approximately 8.37 GB. The retry passed, as recorded above; the original
failure was not a hash incompatibility.
