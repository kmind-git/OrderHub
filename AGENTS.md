# Agent Instructions

Read [AI_POLICY.md](AI_POLICY.md) and [CONTRIBUTING.md](CONTRIBUTING.md) before making changes.
Follow the [coding standards](docs/developer_guide/coding_standards.md) and the relevant developer
guide for the area you change. Older developer guides may describe the upstream release process;
the local instructions in this file and `CONTRIBUTING.md` govern this checkout.

This is an independently maintained local derivative of NautilusTrader for OrderHub. It is not an
official Nautech Systems repository. Preserve upstream copyright notices, license terms, source
provenance, and existing internal `nautilus-*` crate and API names. Do not route local issues,
security reports, support requests, or releases to upstream maintainers.

**This software can execute live trades involving real capital. Hold every change to a high
standard for correctness, reliability, testing, clarity, and maintainability.**

## Environment and scope

- Use the stable Rust toolchain pinned by `rust-toolchain.toml` for required build, format, lint,
  test, and documentation gates. Do not make nightly a prerequisite for those gates. Optional
  experiments which inherently require nightly must be identified separately and cannot replace
  stable validation.
- Supported build, test, schema and repository-check entries use Rust and native tools. Do not
  reintroduce Python/PyO3 features or required Python environments. An ignored historical
  `python/.venv` may remain locally and is not a supported project environment.
- Read affected code and search for existing patterns before changing it. Keep changes focused and
  match the established style, types, and dependencies.
- Preserve exact arithmetic for prices, quantities, money, and fees using domain types or `Decimal`.
- Keep scripts and paths portable across Linux, macOS, and Windows. Normalize path separators with
  shared helpers before splitting or joining paths.
- Change generated artifacts through their source and generator. Do not hand-edit them.
- Preserve unrelated local edits. Do not rewrite Git history or change remotes as part of a code
  change unless the user explicitly requests it.

## Tests and validation

- Do not add test-only production behavior or weaken tests to make a check pass.
- Run the smallest relevant tests while developing. Before presenting a change as ready, run the
  relevant formatting, lint, and tests described in `CONTRIBUTING.md` and report any unavailable
  checks accurately. Validation applies to the exact revision tested.
- Tests must not depend on adapter environment variables; `make pre-flight` strips them through
  `scripts/strip-adapter-env.bash`. Register new adapter variables there.
- Redis and PostgreSQL integrations remain supported where configured. Do not remove them while
  simplifying upstream CI or release automation.
- For the dedicated `orderhub-tools` database entry, validate `schema-check`, argument handling,
  formatting, lint, and tests without assuming a PostgreSQL service is available. The general
  CLI is retired; preserve the versioned offline historical converter in
  `orderhub-tools/legacy-catalog` and its original-file compatibility tests.
- Use `orderhub-check` for dependency feature and test network policies. Compile the migrated
  Redis bus and Cache restoration tests in `nautilus-infrastructure`; service execution remains
  a separate activation gate.

## Repository and public interaction

- Do not commit, amend, push, publish, deploy, or interact with remote issues or pull requests
  unless the user explicitly requests that action.
- Keep local review descriptions specific and evidence-based. Do not claim an upstream release,
  upstream support, or a successful check that did not run.
- Preserve the existing license and attribution. AI assistance does not transfer authorship or
  licensing rights; see [AI_POLICY.md](AI_POLICY.md).
