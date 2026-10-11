# SQLx 0.9.0 custom rustls provider prototype

These four crates are copied from the locally cached crates.io SQLx 0.9.0 release. Their
original `Cargo.toml.orig` and MIT/Apache-2.0 license files are retained alongside the source.
This is a local OrderHub patch, not an upstream SQLx release. It is exercised by
`prototypes/orderhub-sqlx-tls` and is not yet wired into the root workspace.

Original registry `Cargo.toml` SHA-256 values:

| Crate | SHA-256 |
| --- | --- |
| `sqlx` | `23240E25CD000469FD426239DB3C48532B72F290A01C1C151DC306408AD9EC8D` |
| `sqlx-core` | `D62BB81E87B97948B8A24C75D5C17519FB183F1A837085D76DDF30F0AB9B65BE` |
| `sqlx-macros` | `C9C80428D346C752A0F4B19994D88DA5B7458BF736A145027EA129845B085632` |
| `sqlx-macros-core` | `12E43F58C57B7CCBEBB4CF2EA5A1B68A4510A1AB3D6F7FCE0E58C151625D29B3` |

The `tls-rustls-custom-provider` feature is forwarded through the facade, macros, and core.
The core's TLS handshake uses the installed rustls provider and returns a configuration error
if none is installed. The pre-existing AWS-LC and ring branches remain available for unrelated
SQLx consumers, but OrderHub must never activate them in its no-C feature matrix. Two upstream
modules were gated by the features that actually use them so this local path can pass the
repository's warning-deny rule with `derive` alone. No certificate or PostgreSQL behavior was
removed from SQLx.

Do not use SQLx's default `PgSslMode::Prefer` for the intended PostgreSQL TLS configuration:
it can use plaintext when the server declines TLS. Set `VerifyFull` and provide a trusted root.
The prototype tests valid and invalid certificate paths, a server's TLS refusal, and explicit
plaintext mode against a local protocol fixture. A full PostgreSQL service test with queries,
transactions, authentication, pooling, and reconnection is required before enabling the
PostgreSQL service for business use.
