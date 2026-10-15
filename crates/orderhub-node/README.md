# OrderHub node

This local node assembles the OrderHub bridge, journal, native risk/execution engines, and gRPC
server. It uses the pinned stable Rust toolchain and needs the local FIX exchange for execution.
Current business capabilities and activation gates are defined in the
[simplification plan](../../orderhub-project-simplification.md).

## FIX session and logs

`ORDERHUB_FIX_CONFIG` selects the single-session file; the default is `config/orderhub-fix.cfg`
relative to the working directory. The file owns the endpoint, CompIDs, HeartBtInt and diagnostic
log settings. `ORDERHUB_EXCHANGE_ADDR` is retired and rejected. Configuration is validated before
the business journal opens. See [FIX configuration and logs](../../docs/orderhub/local-exchange.md).

With logging enabled, a dedicated writer produces `messages.current.log` and `event.current.log`
under FileLogPath, prefixed with the FIX session identity. FileLogPath is relative to the config
file; rotation defaults to 10 MiB per file and five backups. Diagnostic queue overflow or runtime
file errors are reported without waiting for disk on the order path. These logs are not the
business recovery journal.

## Committed business state

Admission, reservations, request results, execution facts, and authorized event views are committed
in the local redb journal. The [P5 runtime guide](../../docs/orderhub/business-runtime.md) describes
the supported capabilities, exact ledgers, queue limits, and failure behavior.

Startup verifies the sealed recovery chain and rebuilds native Cache orders and execution positions.
Historical input facts do not establish live freshness. A journal with historical business orders
stays HALTED until complete external reconciliation is available; the local simulator has no such
query coverage. Original IDs, results, and unresolved reservations are retained. Historical
unversioned journals or execution facts lacking native material require explicit offline enrichment.

Ctrl+C drains the core for up to 250 ms and publishes an immutable checkpoint under
`ORDERHUB_CHECKPOINT_DIR` (default: the journal path with extension `checkpoints`). The offline
`orderhub-recovery` binary provides inspect, upgrade, backup, and restore operations; restoration
requires a new target and fences a new epoch. See the [P6 recovery guide](../../docs/orderhub/recovery-runtime.md)
for commands, failure behavior, and deployment limitations.

## Market and account inputs

The node uses a running `LiveClock`. No account balance or instrument definition is inferred at
startup. New orders stay blocked until a configured adapter supplies fresh, complete, versioned
inputs. `GetState.writable` includes this input gate.

`ORDERHUB_INPUT_STDIN=1` enables a continuous JSONL pipe containing explicit `local-fixture-v1`
facts for AAPL.GOX / GOX-001 / ORDERHUB-S001. This is a local fixture source, not an exchange or
broker account source. Trading-status and account snapshots expire after 5 and 10 seconds.
Quotes remain in memory; quote age or continuity does not gate new orders. EOF, malformed framing,
queue loss, or missing required snapshots close the gate. Producers stamp
source event time and sequence; the node stamps local receipt time. The historical deterministic
test fixture cannot be replayed unchanged as a fresh live input.

See the [input runtime guide](../../docs/orderhub/input-runtime.md) for the schema and
resynchronization behavior. PostgreSQL/Redis services are not required for this input path.

Supported formatting, lint, and tests are documented in [CONTRIBUTING](../../CONTRIBUTING.md).
