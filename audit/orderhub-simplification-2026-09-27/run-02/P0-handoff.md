# P0 handoff after stable toolchain migration

Date: 2026-09-27. Target: `x86_64-pc-windows-msvc`, Rust 1.98.1 stable.
This run supplements `../run-01/P0.md` rather than replacing its historical results.

## Repository change and checks

- Normal Rust format, check, Clippy, tests, and node build now use the pinned stable toolchain.
  `rustfmt.toml` contains only stable options; the current CI installs `rustfmt` with that toolchain.
- The Windows MSVC target allows only the informational `linker_messages` lint produced when
  the linker announces `.dll.lib` and `.dll.exp` creation. `build.warnings = "deny"` and the
  Clippy `-D warnings` gate remain in force for all other warnings.
- `cargo fmt --all -- --check`: passed; `stable-format-after.log` is empty.
- `cargo check --locked -p orderhub-gateway -p orderhub-node -p orderhub-proto --all-targets`:
  passed (`check.log`). The same package selection for Clippy with `-D warnings` passed
  (`clippy.log`).
- `cargo test --locked -p orderhub-gateway -p orderhub-node -p orderhub-proto --lib --bins`:
  passed, six tests (`unit.log`). The isolated `m0_validation`, `m1_validation`, and
  `m2_validation` integration targets passed 6 + 5 + 6 tests (`isolated-integration.log`).
- `cargo build --locked -p orderhub-node --bin orderhub-node`: passed (`node-build.log`).
- `git diff --check` for touched tracked files: passed. PowerShell parsed the local idle-node
  measurement script with zero errors. Git Bash could not start its signal pipe in this managed
  Windows environment (`Win32 error 5`), so `bash -n scripts/regen-capnp.sh` did not run.

## Resource baseline

`measure-idle-node.ps1` started only the newly built local node. It used a new redb file in this
run directory, an ephemeral loopback gRPC listener, and an ephemeral loopback FIX acceptor that
sent a synthetic logon and no order or market-data messages. The script stopped the node and
acceptor after ten one-second samples. `idle-node-rss.csv` records a 19,222,528-byte peak working
set (about 18.3 MiB); the new empty journal is 1,056,768 bytes. No OrderHub node process remained
afterward. This is an **idle startup baseline only**, not the 2 GiB capacity fixture, A12 exchange
test, recovery proof, or a demonstration of safe READY behavior.

## P0 exit and P1 entry

All 26 current workspace members have a disposition in the plan. The Windows dependency graph,
key hashes, build/link candidates, entrypoint references, schema and historical file list, fixed
numeric targets, host context, idle resource sample, and baseline successes/failures are recorded
across `run-01` and `run-02`. P0's scope and evidence gate is complete for the selected Windows
target. The unfiltered offline metadata command still lacks cached Linux packages; the
Windows-filtered metadata succeeded. The simulated exchange and database services were not run.

P1 C0 inventory may proceed. `native-candidate-workspace-consumers.csv` is a broad reverse reachability
map from the locked Windows metadata, not proof that each candidate compiles for every consumer.
The targeted dependency trees and later native-link evidence must decide actual inclusion.
The first attempt to query crates.io for `rustls-graviola` and `graviola` failed with Schannel
`SEC_E_NO_CREDENTIALS`; no C1 dependency version or TLS handshake was locked or accepted.
