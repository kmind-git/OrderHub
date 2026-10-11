# HTTPS TLS adapter probe

This standalone crate verifies reqwest 0.12's `rustls-tls-no-provider` selection with a local
HTTPS server. It installs the Graviola provider, supplies a test root certificate, receives an
HTTP response, and rejects a wrong-host certificate. It does not cover external endpoints,
client certificates, proxy settings, timeouts, or reconnection.

Run with the pinned stable toolchain:

```powershell
cargo fmt --manifest-path prototypes/orderhub-https-tls/Cargo.toml -- --check
cargo clippy --offline --locked --manifest-path prototypes/orderhub-https-tls/Cargo.toml --all-targets -- -D warnings
cargo test --offline --locked --manifest-path prototypes/orderhub-https-tls/Cargo.toml --lib
```
