# OrderHub design principles

These principles guide changes to the current OrderHub runtime. See
[business runtime](../orderhub/business-runtime.md) for implemented scope and
[contribution gates](../../CONTRIBUTING.md) for validation requirements.

## Correctness and exact arithmetic

Represent prices, quantities, money, fees, and reservations with domain types or Decimal.
Validate capability, precision, identity, and bounds at the relevant boundary. Never substitute
floating-point approximations for business state.

A retained engine feature or domain type does not enable that capability in the node.
Document node behavior separately from library capabilities.

## Runtime ownership

The core thread owns mutable trading state and Cache access. gRPC handlers send value objects
through bounded channels and receive responses; they must not block on or directly access Cache.
Keep command ordering, event processing, and lifecycle transitions explicit.

See [admission and idempotency](../orderhub/admission-and-idempotency.md) and
[state and event stream](../orderhub/state-and-event-stream.md).

## Bounded work

Use explicit queue capacities and admission behavior under load. Account for in-flight commands,
reserved funds and positions, event retention, and projection lag. Report rejection or halted
state when a safety boundary is reached; silently dropping accepted work is unacceptable.

Use the workload and limits documented in [venues and capacity](../orderhub/venues-and-capacity.md).
A microbenchmark does not establish end-to-end capacity.

## Persistence and recovery

Persist the business transition at its defined commit boundary. The local redb ledger records
business state and events; in-memory Cache is a runtime view. A failure between persistence and
external execution requires the documented recovery and reconciliation process.

Recovery must resolve uncertain venue effects before admitting new orders. Do not replay
historical order submissions as new live commands. Preserve idempotency keys, event sequence
numbers, reservations, and recovery fences.

See [storage and commit](../orderhub/storage-and-commit.md),
[recovery runtime](../orderhub/recovery-runtime.md), and
[recovery and reconciliation](../orderhub/recovery-and-reconciliation.md).

## Integration boundaries

Keep transport, venue connectivity, and optional storage integrations behind explicit interfaces.
Redis and PostgreSQL remain optional and require their own service activation checks.
Keep protocol tests, local runtime checks, and real service validation distinct.

See [input runtime](../orderhub/input-runtime.md),
[local exchange](../orderhub/local-exchange.md), and [TLS](../orderhub/tls.md).

## Evidence and maintainability

Prefer the smallest implementation and check that cover the actual failure mode. Keep tests
deterministic where practical, use real module boundaries for integration checks, and record
the exact revision and remaining limitations. Follow [testing](testing.md) and
[coding standards](coding_standards.md).

Preserve third-party source notices and stable internal identifiers. Review compatibility when
changing persisted schemas, protocol fields, or generated artifacts.
