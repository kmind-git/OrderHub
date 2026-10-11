# nautilus-infrastructure

Retained Rust database and messaging backends for the independently maintained local
OrderHub derivative. This checkout is not an upstream release or support channel.

The `nautilus-infrastructure` crate provides backend database implementations and message bus adapters
retained from NautilusTrader:

- **Redis integration**: Cache database and message bus implementations using Redis.
- **PostgreSQL integration**: SQL-based cache database with full data models.
- **Connection management**: Connection handling with retry logic and health monitoring.
- **Serialization options**: Support for JSON and MessagePack encoding formats.

The crate supports multiple database backends through feature flags, allowing users to choose
the appropriate infrastructure components for their specific deployment requirements and scale.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `postgres`: Enables the PostgreSQL SQLx models and cache database backend.
- `postgres-tls`: Adds verified TLS through the local SQLx custom-provider patch.
- `redis` (default): Enables the Redis cache database and message bus backing implementations.
- `tls`: Enables the shared Graviola provider and CPU capability validation.

The Redis feature selects `rustls-graviola` for TLS without a C cryptographic backend.
TLS connections check the required CPU features and fail if another rustls provider was
installed first. Plain Redis connections do not initialize a TLS provider. Plain PostgreSQL
builds do not select this provider. PostgreSQL TLS uses explicit roots and `VerifyFull`;
supported connection factories reject permissive TLS and downgrade modes.
Real Redis/PostgreSQL authentication, transactions, pooling, Streams and reconnect behavior
require separate validation before activation. The OrderHub node rejects activation flags
until that gate passes. Local TLS listeners do not count as real service validation.

## Documentation

See [local TLS configuration](../../docs/orderhub/tls.md) and
[local tooling](../../docs/orderhub/tooling.md). Upstream documentation describes source
history and may not match this derivative.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
