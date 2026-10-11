# OrderHub roadmap

OrderHub uses NautilusTrader as its trading core. The original
upstream roadmap and Kanban board do not track this project's commitments.

The current design direction is documented in [docs/orderhub](docs/orderhub) and
[orderhub-oms-transformation.md](orderhub-oms-transformation.md). Those documents distinguish
design proposals from completed implementation. Priorities include:

1. Reliable admission, submitter identity, idempotency, and bounded risk checks.
2. Durable order and event state with restart recovery and venue reconciliation.
3. Measured capacity and controlled venue integration for the intended deployment.
4. Reproducible local tests and operational documentation for the exact implemented behavior.

Changes to scope and release timing are decisions for this local project's owner. This document
does not imply that the planned capabilities are production-ready.
