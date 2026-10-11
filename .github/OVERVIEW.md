# Local CI overview

The GitHub Actions workflow in this checkout validates source changes on a GitHub-hosted runner.
It does not publish packages, merge branches, dispatch documentation builds, or require upstream
secrets or self-hosted runners. The previous upstream release and wheel workflows are retired.

- [`ci.yml`](workflows/ci.yml) runs Rust formatting, workspace compilation, focused OrderHub
  Clippy and local tests. It does not start the external `rust-trader` exchange process.
- The same workflow audits `Cargo.lock` with pinned `cargo-audit` and the local advisory policy.

These checks are a development gate, not evidence that a particular deployment is ready to trade.
The broader Redis, PostgreSQL, venue, and recovery tests still need suitable local environments.
The workflow has not run on GitHub for this local revision.
