# OrderHub documentation style

Use [Markdown style](markdown_style.md) for syntax and formatting.
The repository's `.markdownlint.jsonc` enforces its mechanical subset.

## Documentation locations

- The root [README](../../README.md) describes supported behavior and startup.
- [docs/orderhub](../orderhub) contains business contracts, runtime behavior, tooling, and plans.
- [docs/developer_guide](.) contains development, coding, and validation instructions.
- `audit/` records historical review and acceptance evidence for specific revisions.
- Rust module documentation describes retained library APIs.

Update [the documentation index](../README.md) when adding an active guide.
Keep proposed functionality explicitly labeled as a plan. A historical audit is evidence for
its recorded revision; it does not describe every later revision.

## Content and tone

Give each page one main purpose: a task, an explanation, a contract, or a reference.
Use concrete behavior, active voice, and familiar terms. Chinese and English documentation
are both supported. Preserve external protocol names and internal API identifiers.

State the implemented scope, prerequisites, steps, and failure behavior where relevant.
Describe the checks actually performed and any unverified integration or deployment condition.
Avoid claims of production readiness based only on compilation or local tests.

Keep feature flags and command examples consistent with their current manifests.
Do not document removed commands or link users to another project's support or release process.

## Examples and links

Use runnable commands with required arguments. Use placeholders for secrets and private paths.
Document whether a command writes files, needs a service, or opens network connections.

Prefer relative links to current repository files and verify that their targets exist.
For historical source, link to the exact Git revision so the reference survives later cleanup.
Use stable section anchors and descriptive link labels.

## Tables and diagrams

Use tables for comparisons and mappings. Keep column padding consistent with the Markdown guide.
Use diagrams when they clarify ownership, event flow, or recovery. Distinguish persisted state,
runtime views, and external systems.

Generated tables must be updated through their source and generator. The execution engine's
[order denied reasons](../orderhub/order-denied-reasons.md) documents its generation command.

## Validation

Run Markdown lint and `git diff --check` for changed documents. Check internal links after
moving or deleting a target. Validate generated artifacts with their source tests.
Follow the relevant gates in [CONTRIBUTING.md](../../CONTRIBUTING.md).
