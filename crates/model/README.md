# nautilus-model

Trading domain types used by OrderHub.

Identifiers, instruments, accounts, orders, positions, and events, with exact price, quantity,
and money types. The node supports a narrower contract than the complete domain model.

## Feature flags

- `defi`: Enables retained DeFi domain types.
- `ffi`: Enables the retained C interface.
- `high-precision`: Uses 128-bit fixed-point price and quantity values.
- `test-support`: Enables domain fixtures and builders.

## Project documentation

- [OrderHub overview](../../README.md)
- [Current business runtime](../../docs/orderhub/business-runtime.md)
- [Development and validation](../../CONTRIBUTING.md)

Build local API documentation with `cargo doc --locked -p nautilus-model --no-deps`.

## Source notices

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
The retained source is licensed under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
See each source file for its copyright and license notices.
