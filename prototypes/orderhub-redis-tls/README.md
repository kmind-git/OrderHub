# Redis TLS adapter probe

This separate workspace depends on the production `nautilus-infrastructure` Redis feature.
It keeps the Redis connection-manager, Sentinel, Streams, and Tokio rustls features enabled.
The local fixture first calls the production Redis connection entry and confirms that its TLS
provider rejects an untrusted certificate. It then checks a successful TLS handshake and RESP
request, wrong-host certificate rejection, and explicitly configured plaintext `redis://`
behavior. The fixture is not a Redis server and does not verify authentication, connection
management, Sentinel, Streams, timeouts, or reconnection.

Run with the pinned stable toolchain:

```powershell
cargo fmt --manifest-path prototypes/orderhub-redis-tls/Cargo.toml -- --check
cargo clippy --offline --locked --manifest-path prototypes/orderhub-redis-tls/Cargo.toml --all-targets -- -D warnings
cargo test --offline --locked --manifest-path prototypes/orderhub-redis-tls/Cargo.toml --lib
```
