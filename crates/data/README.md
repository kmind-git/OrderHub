# nautilus-data

Market data engine components used by OrderHub.

Data clients, subscriptions, aggregation, and order book updates. The supported node input
contract is documented in the OrderHub input runtime guide.

## Feature flags

- `defi`: Enables retained DeFi domain types.
- `high-precision`: Uses 128-bit fixed-point price and quantity values.

## Project documentation

- [OrderHub overview](../../README.md)
- [Current business runtime](../../docs/orderhub/business-runtime.md)
- [Development and validation](../../CONTRIBUTING.md)

Build local API documentation with `cargo doc --locked -p nautilus-data --no-deps`.

## Source notices

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
The retained source is licensed under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
See each source file for its copyright and license notices.
