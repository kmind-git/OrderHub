# How-to Guides

This local OrderHub derivative uses the pinned stable Rust toolchain. Start with the
[repository README](../../README.md) and [contribution guide](../../CONTRIBUTING.md).
Current business capabilities and recovery boundaries are documented in the
[OrderHub guides](../orderhub/business-runtime.md).

## Retained workflows

| Guide                                                               | Description                                                      |
| ------------------------------------------------------------------- | ---------------------------------------------------------------- |
| [Write an actor in Rust](write_rust_actor.md)                       | Use the retained DataActor API for subscriptions and handlers.   |
| [Convert historical Parquet](migrate_parquet_catalog.md)            | Read supported original files into a versioned offline artifact. |
| [Recover a local business journal](../orderhub/recovery-runtime.md) | Inspect, checkpoint, back up, and restore the OrderHub ledger.   |
| [Maintain database schemas](../orderhub/tooling.md)                 | Use the dedicated Rust schema tool and repository checks.        |

## Historical guides

The following pages describe retired upstream capabilities. Their commands are not supported
execution entries in this checkout; they remain available as historical reference.

- [Configure a live trading node](configure_live_trading.md)
- [Get started with Lighter](get_started_lighter.md)
- [Write a Rust strategy](write_rust_strategy.md)
- [Run a Rust backtest](run_rust_backtest.md)
- [Run Rust live trading](run_rust_live_trading.md)
- [Stream a Parquet catalog](stream_parquet_catalog.md)

See [framework retirement](../orderhub/framework-retirement.md) for retained replacements and
source provenance. The data actor API remains available; the general strategy and node
frameworks described in the historical pages are retired.
