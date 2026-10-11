# nautilus-common

Common componentry for [NautilusTrader](https://nautilustrader.io).

The `nautilus-common` crate provides shared components and utilities that form the system foundation for
NautilusTrader applications. This includes the actor system, message bus, caching layer, and other
essential services.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `defi`: Enables DeFi (Decentralized Finance) support.
- `high-precision`: Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `live`: Enables the Tokio async runtime for live trading.
- `simulation`: Enables deterministic simulation testing with
  [MadSim](https://crates.io/crates/madsim).
- `tracing-bridge`: Enables the `tracing` subscriber bridge for log integration.

## Documentation

See [the docs](https://docs.rs/nautilus-common) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
