# P1 progress: Windows TLS candidate and C0 inventory

Date: 2026-09-27. Platform: `x86_64-pc-windows-msvc`. Toolchain: pinned Rust 1.98.1 stable.
P1, C0, and C1 remain open; this report records a bounded probe, not the final no-C gate.

## Stable requirement

`AGENTS.md` now makes the pinned stable toolchain the required Rust build/format/lint/test/docs
baseline. `CONTRIBUTING.md`, `Makefile`, `rustfmt.toml`, `scripts/regen-capnp.sh`, and current CI
were adjusted in run-02. Optional Miri/nightly-specific experiments are outside required gates.

## Candidate source and scope

The standalone `prototypes/orderhub-no-c-tls` crate fixes `rustls=0.23.45` without default
features, `rustls-graviola=0.4.0`, and the lock selects `graviola=0.4.1`.
`Cargo.lock` SHA-256: `9638B7D14388C81AC37ED1D4BB01DDCC86FEDA8F57AC106889AAC86DFBF51513`.
Locally cached registry manifests for `rustls-graviola` and `graviola` hash to
`205E6861D8FDDA36E6670D0F45A6D6C2DEE158818F662B2C7B4903AE155241B8` and
`3A3C40ABA3D66ABE04AAA7580C19EB4FD0E7DE75CEF3DF4D80C847692655BB8E` respectively.
The original six fixture hashes can be obtained with
`Get-FileHash prototypes/orderhub-no-c-tls/fixtures/*`; the fixtures were regenerated on
2026-09-28 with an IP SAN and an additional wrong-host certificate/key pair. Current fixture
hashes and tests belong to run-04. The private keys are test-only.

The normal/build dependency tree contains `rustls`, `rustls-webpki`, `rustls-graviola`,
`graviola`, and ordinary Rust dependencies, with no AWS-LC, ring, cc, cmake, zstd, or mimalloc.
An unfiltered Cargo metadata package list also contains `ring` because registry packages declare
their own development dependencies; that list does not imply the probe builds ring.
The selected Windows normal/build tree and the fresh verbose build show no ring compilation.
`getrandom` and `rustls` do have Rust build scripts; these are not third-party C implementations.

The fresh build used a previously absent target directory under this run and pointed `CC`,
`CXX`, `CMAKE`, and `NASM` at `prototypes/orderhub-no-c-tls/compiler-sentinel.cmd`, which exits
99 if invoked. `cargo test --offline --locked -vv` exited 0. Its seven tests passed:
TLS 1.2/1.3 ECDSA and RSA handshakes; wrong-host, untrusted, and expired certificate rejection.
The full verbose output and exit status are in `fresh-tls-test.txt` and `fresh-tls-exit.txt`.
The sentinel and tree do not by themselves prove absence of direct compiler calls, static link
inputs, or runtime dynamic loading; final C6 requires those checks.

## Current C0 mapping for actual Windows normal/build paths

| Current selection | Observed candidate | Planned action | Gate |
| --- | --- | --- | --- |
| `orderhub-node`, `orderhub-gateway`, `orderhub-proto` defaults | No AWS-LC/ring/zstd/mimalloc in the captured normal/build trees | Repeat after feature wiring | C2/C6 |
| `nautilus-infrastructure` default Redis | `aws-lc-sys` through unconditional cryptography dependency | Decouple signing and unify TLS provider; preserve Redis TLS | C2/C3 |
| `nautilus-infrastructure` with `postgres` | SQLx 0.9, AWS-LC via cryptography; PostgreSQL TLS feature is not yet enabled | Patch SQLx provider selection and test both plain and TLS | C2 |
| `nautilus-cryptography` | `aws-lc-sys` | Replace necessary hash/HMAC; decide RSA consumer scope | C3 |
| `nautilus-persistence` | `zstd-sys` | Preserve required history through Rust codec/migration | C4 |
| `nautilus-testkit` | `aws-lc-sys`, `ring`, `zstd-sys` | Remove test-path C entries without reducing coverage | C3/C4/C5 |

The full captured trees are `*-normal-build-tree.txt` and `infrastructure-postgres-tree.txt`.
`infrastructure-tree.txt` includes development dependencies and is deliberately separate.
This is a partial C0 inventory: other retained libraries, tools, examples, benches, external
processes, FFI, and runtime loading still require inspection.

## Remaining P1 gates

- Review candidate security/maintenance and required CPU startup behavior; test system roots,
  mutual TLS if configured, timeouts and reconnects, and peer interoperability.
- Implement feature-safe provider wiring for Redis, SQLx PostgreSQL, gRPC and required HTTPS;
  test valid and invalid certificates with no plaintext fallback.
- Resolve C3-C5 for cryptography, BLAKE3, compression, certificates, generator and test paths.
- Repeat fresh debug/release builds and link/runtime inspections for every final retained target.

No application dependency or production TLS setting was changed by this probe.

## C3 partial code migration

`nautilus-cryptography::signing::hmac_signature` now uses `hmac` 0.13.0 and `sha2` 0.11.0.
Its five existing HMAC-SHA256 fixed vectors all pass, including empty key and data. RSA signing
and the default rustls provider still use AWS-LC; this change does not remove it from the crate.
`nautilus-testkit::files` now uses `sha2` 0.11.0 for streaming file checksums and test byte
digests. The direct `aws-lc-rs` dependency was removed from testkit's `datasets` feature, but
other dependencies still bring AWS-LC, ring, and zstd into its current default build.

Validation on the changed revision: `cargo test --offline --locked -p nautilus-cryptography --lib`
passed 10 tests; the targeted `test_calculate_sha256` and `test_verify_sha256_checksum` tests
passed. The former compares fixed SHA-256 values for a short file and 4097 bytes crossing the
4096-byte read boundary. `cargo clippy --offline --locked -p nautilus-cryptography
-p nautilus-testkit --lib -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`
for touched tracked files passed. The first HMAC compile failed because `KeyInit` was missing
from imports; it was fixed before these final checks. The full testkit suite, Python bindings,
historical file corpus, and application matrix were not run for this change.
The existing `orderhub-gateway`, `orderhub-node`, and `orderhub-proto` all-targets check also
passed with `--offline --locked` on the resulting root lock file.
