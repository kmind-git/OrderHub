# nautilus-testkit

Test helpers used by OrderHub.

In-memory Cache database doubles and event collectors. Tests use local fixtures.

## Feature flags

- `cache`: Enables the in-memory Cache database double (default).
- `high-precision`: Uses 128-bit fixed-point price and quantity values.

## Project documentation

- [OrderHub overview](../../README.md)
- [Current business runtime](../../docs/orderhub/business-runtime.md)
- [Development and validation](../../CONTRIBUTING.md)

Build local API documentation with `cargo doc --locked -p nautilus-testkit --no-deps`.

## Source notices

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
The retained source is licensed under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
See each source file for its copyright and license notices.
