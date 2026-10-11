# tonic TLS transport probe

This standalone crate exercises tonic server and client TLS over HTTP/2 with the Graviola
provider, a local test root, and wrong-host certificate rejection. Its service returns a fixed
gRPC status for one path. It does not test OrderHub's generated protocol, identity, authorization,
client certificates, timeouts, or reconnection.

Run with the pinned stable toolchain:

```powershell
cargo fmt --manifest-path prototypes/orderhub-grpc-tls/Cargo.toml -- --check
cargo clippy --offline --locked --manifest-path prototypes/orderhub-grpc-tls/Cargo.toml --all-targets -- -D warnings
cargo test --offline --locked --manifest-path prototypes/orderhub-grpc-tls/Cargo.toml --lib
```
