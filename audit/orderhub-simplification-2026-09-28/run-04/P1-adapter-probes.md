# P1 adapter probes: SQLx, Redis, HTTPS, and gRPC

Date: 2026-09-28. Platform: Windows x64, `x86_64-pc-windows-msvc`. Toolchain:
`rustc 1.98.1` stable, as pinned by `rust-toolchain.toml`; full compiler details are in
`toolchain.txt`. All commands below ran from the repository root. P1 remains open.

## Scope and source

- `patches/sqlx-0.9.0` is a local copy of four crates from the cached SQLx 0.9.0 source.
  `README.md` in that directory records the original manifest hashes and the provider feature
  adaptation. The SQLx prototype alone references these patches. Root application dependencies
  have not been rewired.
- `prototypes/orderhub-sqlx-tls`, `orderhub-redis-tls`, `orderhub-https-tls`, and
  `orderhub-grpc-tls` are separate locked Cargo workspaces. Their lock hashes and the eight
  current certificate/key fixture hashes are in `source-hashes.txt`. Fixture private keys are
  test-only. The generator is `prototypes/orderhub-no-c-tls/generate-fixtures.ps1`.
- The candidate remains rustls 0.23.45, rustls-graviola 0.4.0, and Graviola 0.4.1 on this
  Windows x64 machine. This is a compatibility probe, not provider security approval.
  [Graviola's own README](https://github.com/ctz/graviola) describes the project as very new
  and lists required x86-64 CPU features; its
  [compatibility table](https://github.com/ctz/graviola/blob/main/COMPATIBILITY.md) excludes
  older CPUs. The base prototype now checks those features on the current host before use.
  The revised base prototype passed eight tests in a new compiler-sentinel target directory;
  its final output and exit code are `fresh-v3-base-tls-test.txt` and
  `fresh-v3-base-tls-exit.txt`. The preceding v2 fresh attempt failed for lack of disk space
  before running tests and is not used as evidence. No test on an unsupported machine or
  independent provider security assessment was completed.

## Checks completed

| Target | Behavior exercised | Result |
| --- | --- | --- |
| rustls base | TLS 1.2/1.3 RSA/ECDSA; wrong host, untrusted, expired; host CPU preflight | 8/8 tests passed after fixture regeneration and preflight addition |
| SQLx 0.9 local patch | Missing provider fails; `VerifyFull` TLS startup succeeds; wrong host and server TLS refusal fail; explicit `Disable` sends plain startup | 1/1 sequential protocol test passed |
| Redis 1.7 | TLS sends RESP with test root; wrong-host TLS fails; explicit `redis://` sends plaintext RESP | 1/1 sequential protocol test passed |
| reqwest 0.12 | `rustls-tls-no-provider`, test root, HTTPS response, wrong-host rejection | 1/1 test passed |
| tonic 0.14 | TLS client and server, HTTP/2 request and gRPC response status; wrong-host rejection | 1/1 test passed |

For each of the four adapter crates, these commands exited 0:

```powershell
cargo fmt --manifest-path prototypes/<crate>/Cargo.toml -- --check
cargo clippy --offline --locked --manifest-path prototypes/<crate>/Cargo.toml --all-targets -- -D warnings
cargo test --offline --locked --manifest-path prototypes/<crate>/Cargo.toml --lib
```

The SQLx, Redis, HTTPS, and gRPC tests were also repeated with a previously absent target
directory under this run and `CC`, `CXX`, `CMAKE`, `NASM`, and target-specific `CC`/`CXX`
pointing to `prototypes/orderhub-no-c-tls/compiler-sentinel.cmd`. Each fresh test exited 0.
After the first run, review found that the SQLx, Redis, and HTTPS negative fixtures were both
wrong-host and untrusted, so the rejection did not isolate hostname verification. The tests
were corrected to trust the wrong-host certificate and repeated in new `fresh-v2-*` target
directories; all three passed again. Their final-version outputs and exit codes are
`fresh-v2-*-test.txt` and `fresh-v2-*-exit.txt`; the original `fresh-*-*` files document the
earlier, weaker tests. The gRPC negative test already trusted the wrong-host certificate.
The base rustls fresh build was recorded in run-03; the revised base prototype has the
separate v3 fresh evidence above.
The selected normal/build dependency trees are `*-normal-build-tree.txt`. The four adapter
trees had no selected `aws-lc`, `ring`, `zstd`, `mimalloc`, `cmake`, or `cc` package. These
observations are scoped to their locked feature sets and Windows target.
The generated target directories were later removed to recover disk space for workspace
tests. The captured command output and exit-code files remain; rebuilding them will require
new target directories.

## What this does not establish

- The SQLx fixture stops after PostgreSQL startup. No real PostgreSQL authentication, query,
  transaction, pool, or reconnection was exercised. Its `VerifyFull` setting is a prototype
  choice; the application has not been configured to enforce it. SQLx's default preference must
  not be mistaken for the required no-downgrade policy.
- The Redis fixture is not a Redis service. Connection manager, Sentinel, Streams, auth,
  timeout, reconnect, and operational URI/credential semantics remain to be tested.
- The tonic service returns a fixed gRPC status. OrderHub generated messages, service
  authorization, client certificate policy, timeout, and reconnect remain untested.
- The HTTPS probe uses a local self-signed root. System roots, mTLS, proxy, timeout, and
  reconnect remain untested.
- Fresh target directories and compiler sentinels do not establish all link inputs, runtime
  loading, other targets, or the debug/release application matrix. C0, C1, C2, and C6 remain
  incomplete. C3-C5 work is also required before P1 exit.

No P2 work or production TLS setting is enabled by these prototypes.
