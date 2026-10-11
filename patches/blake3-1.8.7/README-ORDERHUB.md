# OrderHub BLAKE3 build-script patch

This directory copies the published `blake3` 1.8.7 crate from the local Cargo registry.
The original `Cargo.toml` SHA-256 is
`B10A23F248F27CEF9B745ADF8B25B2F4B7E0271F171FA268721DF43F2BB25D25` and the original
`build.rs` SHA-256 is
`9EC9B359DB417071B525A459A191B810E3DD79B9708AE66BE4085B2EFD2BA7D9`.

The only source change is in `build.rs`: when Cargo enables `pure`, skip
`c_compiler_support()` and select the existing no-compiler branch. The hash algorithm and Rust
implementation are unchanged. The published license files are retained as `LICENSE_A2`,
`LICENSE_A2LLVM`, and `LICENSE_CC0`; see `Cargo.toml` for the crate's license expression.

This patch is initially used by an isolated prototype. Application integration requires a
fixed-vector and historical-data comparison plus performance measurement.
