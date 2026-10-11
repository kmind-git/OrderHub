# P3 consumer retirement

Date: 2026-10-01. Scope: tools, Python surfaces, and inherited publication helpers.

## Recoverability

`source-before.zip` contains 2297 files and was verified against `source-before-hashes.json`.
`retirement-manifest.json` identifies each file, reason, size, and SHA-256. `retirement-source.zip`
preserves its exact bytes, including hook line-ending migration. Deletion checks both
the current hash and retirement snapshot hash. Restore selected files from either archive into this checkout
after checking for newer edits; do not overwrite the checkout with the whole snapshot.

Upstream notices remain in retained source. The retired Python package and stub generator license
texts are also retained under `LICENSES/`; their original source and notices remain in the archive.

## Necessary tools

- Dependency features and network policy: run-11 retained 9 configuration and 34 network cases.
- Commit policy, tables, doc links, audit policy, workspace inventory: 68 identical-input cases in
  `tool-parity.json`. Error text, warnings, file bytes, exit status, planned flags and report settings
  are compared as appropriate. CLI success text and the generated link header may differ.
- Security `report` controls reporting, never failure suppression. Cargo audit/deny/vet and OSV
  continue to fail on nonzero audit results. This comparison does not run an advisory audit.
- All 13 optional strict Clippy rules remain in Make; Cargo emits JSON diagnostics directly and
  compilation failures remain failures. The old Python presentation layer is retired.
- Native actionlint consumes the single local workflow directly. The inherited wrapper's
  composite-action expansion has no retained composite actions or callers.
- Changed-crate hooks use jq and Cargo metadata for package names and features. This also resolves
  OrderHub package names without inventing a `nautilus-` prefix.
- FFI ownership and Cython deferral rules from the former legacy hook move into `retired-surfaces`.

## Python assertion consumers

| Retired Python group                                                       | Retained consumer or disposition                                                                                                                                                                                                                                                                           |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Prices, quantities, money, identifiers, order events and state transitions | `crates/model/src/types`, `src/orders`, `src/events` and `crates/model/tests`; the typed Rust implementations and their exact-value assertions remain.                                                                                                                                                     |
| Cache and message transport wrappers                                       | `crates/common/src/cache`, `src/msgbus`, `crates/common/tests`; wrapper ownership/import checks are retired with the wrapper.                                                                                                                                                                              |
| Risk and execution wrappers                                                | `crates/risk/tests/risk_engine.rs`, `crates/execution/tests`; admission and exact domain arithmetic remain Rust responsibilities.                                                                                                                                                                          |
| Python live-node facade and binding/module identity                        | Facade and binding-specific tests retire. Rust lifecycle/reconciliation tests remain in `crates/live/tests` and `crates/system`. Nonempty plugin configuration is rejected.                                                                                                                                |
| Python backtests, strategies, analytics and catalog research               | Current OrderHub does not call the Python research API. Rust framework consumers remain for P7. Four backtest integration files and their private fixtures remain; run-12 preserved 63 function names and 147 assertions, and executed 60 tests. This is not a claim of complete Python-suite equivalence. |
| PostgreSQL custom-data and Redis node behavior                             | Local JSON fixture retains wire fields, metadata and equality. Redis two-node/restart cases remain in `crates/live/tests/integration/redis.rs`. Service execution is deferred until activation.                                                                                                            |
| Python API/stub/doc generation and tutorial execution                      | Retire the generators, generated facade outputs and Python documentation programs together. Local development documentation points to Rust and OrderHub tools.                                                                                                                                             |
| OrderHub journal, idempotency, protocol and recovery                       | `crates/orderhub-gateway/src`, `crates/orderhub-node/src`; existing Rust tests remain. P4-P6 contract implementation is still pending, not covered by this retirement claim.                                                                                                                               |

## Inherited publication helpers

The old upstream wheel, sdist, R2, Docker publication, release-attestation, nightly-merge and
upstream sync entrypoints are outside this local derivative's supported workflow. P0 retired
their workflows. P3 retires their remaining manual scripts and tests after inventorying retained
callers. Dedicated database packaging, PostgreSQL bootstrap and its Docker pull helper, stable
tool readers, optional Miri experiments, ABI checks and local Rust policy tools remain.

Generic Python pre-commit environments are retired. Retained local hooks and native formatters,
actionlint, gitleaks, typos, zizmor and taplo keep the relevant Rust/document/security checks.
No Python environment is required by the supported Rust build, tests, schema or checking tools.
Native tool availability is reported separately; removing a tool's Python installation wrapper
does not count as having run that tool.

The English-only editorial hook is manual because this local project uses Chinese contracts and
Unicode regression fixtures. Business, security and compiler checks keep their failure behavior.
The migrated table formatter preserves the old UTF-16 mode for comparison; documentation hooks
select the new display-width mode to match current MD060 for CJK and emoji.

## Deferred evidence

Real PostgreSQL/Redis behavior remains an activation gate. Missing large ITCH dataset execution
and research data compatibility remain tracked for P7. TLS final application integration and all
retained-path native build/link/runtime evidence remain P4-P8 work. P3 completion does not close
those gates or claim readiness for live capital.
