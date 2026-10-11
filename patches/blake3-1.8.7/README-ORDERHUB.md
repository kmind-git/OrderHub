# OrderHub BLAKE3 pure-only patch

This directory copies the published `blake3` 1.8.7 crate from the local Cargo registry.
The original `Cargo.toml` SHA-256 is
`B10A23F248F27CEF9B745ADF8B25B2F4B7E0271F171FA268721DF43F2BB25D25` and the original
`build.rs` SHA-256 is
`9EC9B359DB417071B525A459A191B810E3DD79B9708AE66BE4085B2EFD2BA7D9`.

The OrderHub workspace and isolated prototype use this patch with `std` and `pure` enabled.
`build.rs` requires `pure` and rejects `neon`, whose upstream implementation needs C. It selects
the existing Rust SSE2/SSE4.1/AVX2 intrinsics on x86, the optional Rust SIMD path on wasm32, and
the portable Rust implementation on other architectures. It does not probe or invoke a native
compiler, mutate `CFLAGS`, inspect the historical `c/` directory, or enable native FFI backends.

The 12 external assembly files and four CMake files (including the package config template) in
`c/` are retired. Remaining upstream native headers, examples, metadata, and documentation are
historical references and are not a supported build entry. The upstream Cargo manifests are
retained for provenance; their declared `cc` build dependency has no compiler calls in this
patch. Enabling BLAKE3 without `pure` fails explicitly instead of selecting a missing backend.

The hash algorithm and Rust implementation under `src/` are unchanged. The published license
files are retained as `LICENSE_A2`, `LICENSE_A2LLVM`, and `LICENSE_CC0`; see `Cargo.toml` for the
crate's license expression. The original build-script hashes above and Git history preserve the
source provenance and the retired build paths.

Relevant regression checks are the fixed digest vectors in
`prototypes/orderhub-blake3-pure`, the OrderHub gateway/node tests, and the original 64/128-bit
Parquet conversion fixtures under the `orderhub-tools` `legacy-catalog` feature. This cleanup
keeps the same `pure` implementation already selected by OrderHub; it adds no new hash algorithm
or performance claim. Cross-platform build-script selection checks do not establish execution
on those platforms.
