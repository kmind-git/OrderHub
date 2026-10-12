# OrderHub tool configuration

`tools.toml` pins native development and security tools used by OrderHub's scripts and
`orderhub-check`. Maintainers update this local catalog directly.

The root `tools.toml` contains optional nightly experiments. Required validation uses the
stable Rust version in `rust-toolchain.toml`.

`scripts/tool-version.sh` and `scripts/cargo-tool-version.sh` read this catalog.
Set `ORDERHUB_TOOL_CATALOG` to an explicit catalog path when exercising the shell consumers
against a separate fixture. Catalog origins and previous revisions remain in Git history.
