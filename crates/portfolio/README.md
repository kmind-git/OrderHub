# nautilus-portfolio

Portfolio management and risk analysis for [NautilusTrader](https://nautilustrader.io).

The `nautilus-portfolio` crate provides portfolio management capabilities including
real-time position tracking, performance calculations, and risk management. This includes
sophisticated portfolio analytics and multi-currency support:

- **Portfolio tracking**: Real-time portfolio state management with position and balance monitoring.
- **Account management**: Support for cash and margin accounts across multiple venues.
- **Performance calculations**: Real-time unrealized PnL, realized PnL, and mark-to-market valuations.
- **Risk management**: Initial margin calculations, maintenance margin tracking, and exposure monitoring.
- **Multi-currency support**: Currency conversion and cross-currency risk exposure analysis.
- **Configuration options**: Flexible settings for price types, currency conversion, and portfolio behavior.

The crate handles complex portfolio scenarios including multi-venue trading, currency conversions,
and sophisticated margin calculations for both live trading and backtesting environments.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `extension-module`: Builds as a Python extension module.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Documentation

See [the docs](https://docs.rs/nautilus-portfolio) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
