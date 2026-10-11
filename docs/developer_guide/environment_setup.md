# Environment Setup

This independently maintained OrderHub derivative uses the stable Rust toolchain pinned in
`rust-toolchain.toml`. Required builds, formatting, lint and tests do not require nightly or Python.

## Platform prerequisites

- Windows: Rust MSVC target and Visual Studio Build Tools with the Windows SDK/linker. Run native
  Cargo commands from PowerShell. Bash helpers additionally need Git Bash and jq on PATH.
- Linux: Rust, a system linker, clang, pkg-config and Git. Make/Bash helpers need Bash and jq.
- macOS: Rust, Xcode command line tools and Git. Make/Bash helpers need Bash and jq.
- Supported schema generation uses Rust/prost. The former Cap'n Proto compiler and research
  serialization build are retired in P7.

Run `rustup show active-toolchain` and `cargo --version` from the repository root. rustup reads the
pin automatically. The optional Miri experiment has a separate nightly pin in `tools.toml` and
does not replace stable validation.

## Local gates

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for required Cargo commands. `make pre-flight` runs
the broader Rust checks where GNU Make is installed. `--profile nextest` keeps assertions and
overflow checks enabled while omitting large debug symbols and incremental caches.
Make targets use this profile by default; select `PROFILE=dev` when debug symbols are needed.
Use `-j 4` on this local Windows host
when rebuilding broad dependency graphs.

`orderhub-check` is a Rust executable. Its checks-only build does not enable the database stack:

```bash
cargo run --locked --profile nextest -p orderhub-tools --no-default-features --features checks \
  --bin orderhub-check -- retired-surfaces
```

Native optional development tools are pinned in the local catalogs. `prek` is the Rust hook runner;
shellcheck, shfmt, actionlint, zizmor, taplo, typos, gitleaks and Markdown formatters run as native
tools. Install the tools for hooks you invoke; report unavailable checks explicitly.

## Retained integrations

PostgreSQL and Redis are retained without third-party C dependencies in their supported paths.
They need no local instances for source checks or schema inspection. Real authentication,
transaction, reconnection and service behavior must be verified before activation. TLS and
certificate validation remain required. See [OrderHub tooling](../orderhub/tooling.md).

An ignored historical `python/.venv` may remain on this computer. It is not used by supported gates.
