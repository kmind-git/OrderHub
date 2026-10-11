# C0 preliminary no-C inventory

This is independent static inventory while P0 remains open. It is not the P1 prototype, does
not prove the final feature union, and does not authorize a backend or module removal.
The locked Windows metadata, package links, and two targeted trees are in this directory.

| Candidate | Observed path or source | Assigned work and verification |
| --- | --- | --- |
| AWS-LC | `nautilus-infrastructure` -> `nautilus-cryptography` -> `aws-lc-rs`/`aws-lc-sys`; Redis/rustls also selects it in the infrastructure tree. | C1/C2/C3: select and evaluate a no-C provider, preserve Redis TLS, replace independent signing/hash uses, then run handshake and native-link checks. |
| SQLx TLS | Root SQLx 0.9 feature set has no PostgreSQL TLS path enabled; current PostgreSQL support is not a TLS acceptance result. | C2: patch source/feature provider selection or migrate driver with query, transaction, pool, error, and TLS parity. |
| ring | Present in locked Windows metadata; absent from the two targeted default trees. Root `reqwest` uses `rustls-tls`; feature union is not yet available. | C2/C5: trace every retained HTTP/test target, select no-provider features, run a complete graph and link check. |
| zstd-sys | Infrastructure dev tree reaches it through persistence/DataFusion/Arrow and zstd. | C4/C5: remove research-only dev coupling; retain and migrate any required historical files with equality checks. |
| mimalloc/libmimalloc-sys | Root dependency, generic CLI, and optional backtest feature. | C4: retire or replace consumers after dedicated Rust administration and performance measurements. |
| libsqlite3-sys | Locked Windows metadata has a native `links=sqlite3` package; targeted node/infrastructure trees did not show it. | C0/C5: identify any supported retained consumer before final graph acceptance; do not infer inclusion from metadata alone. |
| BLAKE3 | Root pins 1.8.7; package has a build script. | C3: inspect the `pure` build branch and verify compiler invocation, digest compatibility, and historical checksums. |
| Cap'n Proto generator | Optional serialization build script calls `capnpc::CompilerCommand`. | C5/P7: remove only after all retained codec and migration consumers have equivalent Rust support. |
| PyO3 and Python tooling | Widespread optional feature and script references; `python/` remains. | P3/C5: migrate necessary generation, schema, test, and operating flows before removing binding code. |

`cc` and `cmake` are build helpers, not evidence of C source by themselves. `aws-lc-sys`,
`zstd-sys`, `libmimalloc-sys`, and `libsqlite3-sys` need inspection of actual compiled inputs.
Rust `extern "C"` ABI declarations are also not third-party C implementations. The final proof
requires fresh isolated builds, build logs, link inputs, and runtime library inspection for
every retained binary, library, tool, and test configuration.
