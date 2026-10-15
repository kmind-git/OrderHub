# OrderHub transport configuration

Implementation date: 2026-10-10. Windows x64 P8 acceptance passed on 2026-10-11.
See the [acceptance record](../../audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)
for build, link, runtime evidence and deferred service activation gates.
The currently selected TLS platform is this Windows x64 development machine. Other platforms,
CPUs, certificate policies and live deployments require their own activation verification.

## Configurations

| Node feature                    | Retained behavior                                                                                   |
| ------------------------------- | --------------------------------------------------------------------------------------------------- |
| none (B)                        | Explicit loopback plaintext node/client; non-loopback addresses and TLS configuration are rejected. |
| redis (R)                       | Retained Redis/TLS production crate, without activating a Redis projection.                         |
| postgres (P)                    | Retained PostgreSQL crate; explicit plaintext options, no opportunistic TLS.                        |
| postgres-tls (PT)               | Includes postgres; patched SQLx uses the shared provider, VerifyFull and explicit roots.            |
| grpc-tls (G)                    | TLS is required at both node and client entries; there is no plaintext fallback.                    |
| redis,postgres-tls,grpc-tls (U) | Shared provider and retained integrations; real services remain disabled.                           |

TLS 1.2/1.3 uses rustls 0.23.46, rustls-graviola 0.4.0 and Graviola 0.4.1.
The rustls patch addresses RUSTSEC-2026-0346 through RUSTSEC-2026-0349; its published-source
review is recorded in `.supply-chain/audits.toml`. Certificate and hostname verification remain
required by this profile.
The shared provider checks real CPU capabilities once before use, rejects another installed
provider and selects X25519, P256 and P384 key exchange. Hybrid ML-KEM is outside this profile.
The backend includes registered inline assembly and requires the documented CPU instructions.

## gRPC TLS

Build both entries with `cargo build --locked --release -p orderhub-node --features grpc-tls`.
Supply PEM files from your selected certificate authority:

- Node: `ORDERHUB_TLS_CERT` (certificate chain), `ORDERHUB_TLS_KEY` (PKCS#8 private key).
- Client: `ORDERHUB_TLS_CA` (trusted PEM bundle), `ORDERHUB_TLS_DOMAIN` (expected server identity).
- Both: `ORDERHUB_ADDR` (explicit IP socket address). The client always uses HTTPS in this build.
- Credentials remain `ORDERHUB_TOKEN` or the node's `ORDERHUB_SUBMITTERS` registry. TLS server
  identity does not grant application access or change submitter/strategy ownership checks.

PEM inputs are bounded to one MiB each. Missing/invalid files and mismatched keys fail explicitly.
The node validates its TLS identity before opening the journal. The recovery configuration digest
includes the transport mode and certificate fingerprint; a changed digest is evidence to review,
and does not independently authorize a recovered node to resume trading.

The fixed certificate/private-key fixtures in infrastructure are public test material.
They are never loaded by the normal entrypoint and must never be used for deployment.
This profile uses server authentication and bearer credentials; mTLS, system roots, proxy support,
revocation policy and certificate reload are separate deployment choices requiring validation.

## Retained databases

`ORDERHUB_REDIS_ENABLED` and `ORDERHUB_POSTGRES_ENABLED` accept only unset, `0` or `false`.
Normal node startup rejects other values before opening business state. Building a feature does
not enable a projection, open a database connection or queue projection tasks.

PostgreSQL options converted from `PostgresConnectOptions` explicitly use `Disable`.
`sql::pg::verified_tls_options` accepts a PEM trust bundle and applies `VerifyFull` using the
configured host. `connect_pg` rejects Allow, Prefer, Require and VerifyCa; verified mode requires
the `postgres-tls` build. SQLx's provider patch preserves certificate and hostname verification.

Redis preserves explicit `redis://` and verified `rediss://` behavior. Its production connection
entry initializes the same provider for TLS. There is no insecure-certificate or fallback option
introduced by this change.

Local protocol listeners exercise TLS/configuration/error propagation; they do not demonstrate
real PostgreSQL/Redis authentication, schema, transactions, pools, messaging or reconnect.
Those remain gates before service activation, as recorded in the
[simplification plan](../../orderhub-project-simplification.md).

## Local checks

```bash
cargo test --locked --profile nextest -p nautilus-infrastructure --no-default-features --features redis,postgres-tls --test tls_provider --test tls_provider_conflict --test postgres_tls --test redis_tls
cargo test --locked --profile nextest -p orderhub-gateway --features grpc-tls --test p8_tls
cargo test --locked --profile nextest -p orderhub-node --features redis,postgres-tls,grpc-tls --test p8_configuration
```

The opt-in node/FIX test supports both B and G. `ORDERHUB_TEST_NODE_EXE` can point to a newly
verified release node for release smoke; otherwise it uses Cargo's just-built binary. Its owned
processes use explicit ports, tokens, fixture inputs and certificate files. The simulator path
is supplied explicitly as described in [recovery runtime](recovery-runtime.md).
