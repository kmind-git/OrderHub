# nautilus-core

Core foundational types and utilities for [NautilusTrader](https://nautilustrader.io).

The `nautilus-core` crate is designed to be lightweight, efficient, and to provide zero-cost abstractions
wherever possible. It supplies the essential building blocks used across the NautilusTrader
ecosystem, including:

- Time handling and atomic clock functionality.
- UUID generation and management.
- Mathematical functions and interpolation utilities.
- Correctness validation functions.
- Serialization traits and codecs.
- Cross-platform environment utilities.
- Abstractions over common collections.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `extension-module`: Builds as a Python extension module.
- `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://crates.io/crates/cbindgen).
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `simulation`: Enables deterministic simulation testing with
  [MadSim](https://crates.io/crates/madsim).

## Documentation

See [the docs](https://docs.rs/nautilus-core) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
