# Security policy for this local checkout

This independently maintained NautilusTrader derivative includes software that can place live
orders. Treat any suspected flaw in order admission, risk limits, execution, persistence,
recovery, credentials, or transport as potentially significant.

## Scope and reporting

This policy covers the code and deployment of this local checkout. It does not cover Nautech
Systems services, the official NautilusTrader repository, or third-party venues and providers.

Report suspected vulnerabilities **privately to the owner or operator of this checkout** using
the private channel already used for this project. Do not post exploitable details in a public
issue. No public contact address, bug bounty, response time, or coordinated disclosure deadline
has been configured for this local project; arrange those explicitly before any public release.

Include the affected revision, deployment context, a minimal reproduction, and the expected
security impact. Avoid using live credentials or customer data in a report.

## Current assurance boundary

The local CI workflow checks Rust formatting, workspace compilation, focused OrderHub tests, and
Cargo dependency advisories.
It does not sign or publish releases, attest packages or container images, or provide a guarantee
that a runtime or deployment is safe for real-money trading. Relevant integration and operational
checks must be run in the target environment.

The original upstream security and supply-chain documentation remains accessible in Git history
as source provenance. Its official advisory inboxes, release attestations, package indexes, and
service commitments do not apply to this checkout.
