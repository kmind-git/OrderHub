# nautilus-execution

Order execution engine components used by OrderHub.

Order routing, lifecycle events, position updates, and execution reconciliation. The supported
node uses the local FIX client; retained engine capabilities do not expand its order contract.

## Feature flags

- `high-precision`: Uses 128-bit fixed-point price and quantity values.
- `simulation`: Enables deterministic simulation support with MadSim.

## Project documentation

- [OrderHub overview](../../README.md)
- [Current business runtime](../../docs/orderhub/business-runtime.md)
- [Development and validation](../../CONTRIBUTING.md)

Build local API documentation with `cargo doc --locked -p nautilus-execution --no-deps`.

## Source notices

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
The retained source is licensed under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
See each source file for its copyright and license notices.
