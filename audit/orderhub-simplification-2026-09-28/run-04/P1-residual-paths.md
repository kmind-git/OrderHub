# P1 residual paths after adapter probes

Date: 2026-09-28. This is a scoped source/dependency inventory, not a completed C0-C6 audit.

| Path | Current evidence | Required disposition before P1 exit |
| --- | --- | --- |
| RSA signing | Repository Rust call sites are `crates/cryptography/src/signing.rs` and its Python binding; no OrderHub Rust caller was found by source search. External Python consumers have not been inventoried. | Preserve until consumer migration or replace with a reviewed no-C implementation and signature vectors. |
| `mimalloc` | Direct uses are the generic CLI and optional backtest examples; OrderHub default paths did not select it in run-03 trees. | Remove or retire those paths with resource comparison; do not treat absence in node default tree as final C4 proof. |
| `zstd-sys` in persistence | Selected via `zstd -> arrow-ipc/arrow -> DataFusion` and Parquet paths in the `nautilus-persistence` normal/build tree. | Protect historical compressed data, then decouple or implement a compatible Rust codec/migration and measure it. |
| `zstd-sys` in testkit | Selected via `zstd -> parquet -> nautilus-testkit` in the normal/build tree. | Migrate required fixtures and checks before removing Parquet/zstd feature paths. |
| `rcgen` | The root workspace dependency still enables `aws_lc_rs`, `crypto`, and `pem`; no direct crate consumer was found in this source search. | Confirm full target/feature use, then remove the unused entry or use controlled fixed fixtures/Rust signing as required. |
| Cap'n Proto compiler | `nautilus-serialization` still has optional `capnpc` behind `capnp`; `scripts/install-capnp.sh` downloads a C++ compiler. | Migrate any required old format, retire obsolete generator/build paths, and verify the remaining protox/prost chain. |
| Fuzz driver | `nautilus-live` has an optional `libfuzzer-sys` feature. | Inventory coverage and migrate necessary cases to a no-C runner or replay before retiring it. |
| PostgreSQL and Redis | Local service/client and Docker/Podman discovery returned no entries in this run. | Run against dedicated real instances with TLS, auth, query/transaction, pool, manager, Sentinel/Streams, timeout, and reconnect cases. |
| SQLx, Redis, HTTPS, tonic TLS | Isolated locked prototypes and certificate tests in `P1-adapter-probes.md` passed, including corrected trusted wrong-host cases. | Wire final application features and unique provider initialization; audit full feature union, identity, errors, downgrade policy, link inputs, and runtime loading. |

This inventory does not authorize deletion of cryptography APIs, Python bindings, storage
formats, or general CLI functionality. The C4/C5 and final C6 matrices remain open.
