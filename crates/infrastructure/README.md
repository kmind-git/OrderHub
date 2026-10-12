# nautilus-infrastructure

Database and messaging backends retained by OrderHub.

Redis and PostgreSQL are optional integrations. Their service behavior requires separate
activation validation; the current node rejects their activation flags. TLS consumers share
the approved provider when its feature is enabled.

## Feature flags

- `postgres-tls`: Enables PostgreSQL TLS through the custom SQLx provider.
- `postgres`: Enables SQLx PostgreSQL models and Cache database support.
- `redis`: Enables Redis Cache and message bus implementations (default).
- `tls`: Enables the shared TLS provider and CPU capability gate.

## Project documentation

- [OrderHub overview](../../README.md)
- [Current business runtime](../../docs/orderhub/business-runtime.md)
- [Development and validation](../../CONTRIBUTING.md)

Build local API documentation with `cargo doc --locked -p nautilus-infrastructure --no-deps`.

## Source notices

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
The retained source is licensed under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
See each source file for its copyright and license notices.
