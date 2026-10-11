# OrderHub tools

Local Rust maintenance tools for the independently maintained OrderHub derivative.

## Feature flags

- `checks`: Enables repository policies, commit messages, Markdown and security audit orchestration.
- `database`: Enables the retained PostgreSQL schema inspection and initialization tool.
- `legacy-catalog`: Optional offline historical Parquet to version 1 native JSONL conversion.
- `high-precision`: Selects 128-bit model values for the offline converter.

`checks` and `database` are enabled by default. The historical converter is opt-in.
A checks-only build avoids the database dependency graph.
Maintenance binary unit tests remain enabled.

See [tooling](../../docs/orderhub/tooling.md) for commands and activation limits. `schema-check`
requires no service. Database initialization is explicit and is verified against a dedicated
instance before activation. Supported tools do not require Python/PyO3.
