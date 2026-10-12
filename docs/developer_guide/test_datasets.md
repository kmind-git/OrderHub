# OrderHub test fixtures

The supported tests use local, versioned fixtures. The optional
[historical converter](../orderhub/tooling.md) retains original-file compatibility coverage.
Source formats and metadata are part of that contract.

## Locations and ownership

- `test_data/` contains shared domain and historical catalog fixtures.
- `crates/*/test_data/` contains fixtures owned by an individual crate.
- `crates/infrastructure/test_data/tls/` contains fixed certificate fixtures.
- `test_data/large/` metadata records dataset origin and integrity; large local data is ignored.

Keep existing fixture paths, original filenames, and source metadata when compatibility tests
depend on them. A historical name in fixture data is not the public project name.

## Adding or changing a fixture

Choose the smallest input that proves the relevant behavior. Store the original source format
when the test checks compatibility or decoding. Record the source, license or usage conditions,
SHA-256 checksum, format, and precision where applicable.

Never include live credentials, customer data, or private account information.
Separate synthetic data from captured protocol or historical source data.

Change generated fixtures through their generator. Run the affected parser, converter, or
restoration tests and record the precision modes exercised.

## Optional external curation

`scripts/curate-dataset.sh` accepts a slug, filename, download URL, and license string.
It uses curl and native hash tools, downloads into `v1/<slug>/` under the current directory,
and writes license and integrity metadata. It does not upload or publish the result.

Run curation from an explicitly chosen artifact directory. Downloading data is a separate
activity from ordinary tests; do not add automatic dataset fetching to a test or required gate.
Review source permissions and metadata before adding a result to the repository.

## Verification

Use the focused native Rust checks described in [CONTRIBUTING.md](../../CONTRIBUTING.md).
The converter's original 64/128-bit file compatibility tests remain required when its fixed-point
conversion changes. Redis restoration tests can be compiled without a service; running them
requires its separate activation gate.
