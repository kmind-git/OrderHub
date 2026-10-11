# OrderHub P0 acceptance thresholds, version 1

Date: 2026-09-27. Scope: local `x86_64-pc-windows-msvc` development build.
These are engineering acceptance targets selected for the planned OMS behavior, not measured
capabilities of the current code. Freeze this version before fault and capacity tests. Any change
requires a new numbered version with the reason, affected tests, and a fresh result.

| Dimension | Target | Measurement and failure rule |
| --- | --- | --- |
| Quote freshness | At most 2 seconds old at admission | Compare source event time and monotonic receipt age with a live clock; missing, future-skewed, disconnected, or stale input blocks new risk. |
| Trading-status freshness | At most 5 seconds old | Unknown, halted, sequence-gap, or stale status blocks new risk for that instrument. |
| Account and position freshness | At most 10 seconds old | Require a versioned snapshot or heartbeat; stale or disconnected input blocks new risk for that account. |
| Clock skew | At most 500 milliseconds between trusted source and node | Greater skew closes new-order admission until a fresh snapshot and clock check pass. |
| Core command queue | At most 4,096 pending; reserve at least 1,024 slots for cancels/reconciliation | Excess new orders receive an explicit capacity error before persistence; cancels retain the reserved path. |
| Execution-event queue | At most 4,096 pending | Overflow halts new risk and raises an observable fault; events cannot be silently dropped. |
| Journal write queue | At most 1,024 pending | Preserve the existing bounded capacity; admission has a finite wait and explicit failure when saturated. |
| Subscriber output | At most 256 buffered events per stream, 128 streams per node | Slow consumers are disconnected with a resumable cursor; replay pages contain at most 1,000 events. |
| Optional projection backlog | First of 10,000 records, 64 MiB, or 30 seconds of oldest-entry age | Stop new risk at the limit; required database loss or lag stops new risk immediately. |
| Process-crash RPO | Zero acknowledged local journal commits lost | Kill before/after commit and reply; verify exact committed IDs after restart. |
| Media-loss backup RPO | At most 15 minutes of local journal history | Restore a consistent backup, then reconcile all uncertain exchange facts before READY. This does not guarantee recovery of a missing external fact. |
| Recovery RTO | At most 30 minutes to READY in the isolated reference environment | Time from process restart through full replay, input refresh, and exchange reconciliation; missing external evidence is a failed run, not an excluded interval. |
| Capacity fixture | 10 submitters, 100 instruments, about 10,000 active orders | Generate 1,000 submissions/s plus 1,000 cancels/s for 10 minutes, i.e. 600,000 of each operation. Do not equate journal-only throughput with this target. |
| Recovery fixture | At least 1,200,000 command records and 10,000 active orders | Record actual bytes, events, identities, outcomes, and schema version before each fault test. |
| Capacity latency | Submit and cancel end-to-end p99 at most 250 ms in the isolated fixture | Measure from gRPC receipt to durable response, including admission, queueing, and journal work; report distribution and rejected requests separately. |
| Resource budget | Peak node RSS at most 2 GiB in the capacity fixture | Measure process RSS and peak disk use, queue depth, and projection lag; no claim until a full run succeeds. |

The numerical freshness and recovery targets are intentionally conservative starting requirements.
They require live input heartbeats, bounded queues, and independent exchange evidence that the
current implementation does not yet provide. Cancels may remain available under a separate
controlled path when new risk is blocked; this does not waive ownership checks.

The reference host currently reports eight logical processors and 36,197,154,816 free bytes on
drive D. Physical RAM discovery was denied, and no running-node resource baseline was available
because the current build gate failed. See `host-baseline.txt` and `P0.md`.
