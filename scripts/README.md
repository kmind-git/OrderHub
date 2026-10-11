# Scripts directory

This directory contains native helpers for the local Rust derivative. Upstream wheel, release,
R2 upload, nightly-merge and Python tooling are retired, with recoverable source in the P3 audit.

`orderhub/package-db-tool.bash` and its PowerShell counterpart package `orderhub-db` and the four
schema SQL files. Schema inspection requires no service. PostgreSQL bootstrap remains for future
activation against a dedicated disposable instance; its Docker pull helper is retained.

`orderhub-check` supplies `dependency-features`, `test-network`, `workspace-tests`, `commit-message`,
`markdown-tables`, `doc-links`, `security` and `retired-surfaces` in Rust. See
[OrderHub tooling](../docs/orderhub/tooling.md). Changed-crate hooks parse Cargo metadata through jq.

`curate-dataset.sh` is a manual helper for curating test fixtures.

---

## `curate-dataset.sh` - package an external dataset locally

`curate-dataset.sh` automates the small but repetitive tasks required when we
prepare a third-party fixture for a mirror controlled by this project:

- download the raw file from its original URL (with retries)
- create a versioned directory (`v1/<slug>/`)
- copy the file into that directory
- write a `LICENSE.txt` file holding the SPDX identifier or license URL
- compute size and SHA-256 checksum and store them in `metadata.json`

The result is a self-contained directory. Review its license and checksum before storing it in
a project-controlled fixture location. This script does not upload it.

### Usage

```bash
scripts/curate-dataset.sh <slug> <filename> <download-url> <license>
```

- **`slug`** - sub-directory name (e.g. `fi2010_all`)
- **`filename`** - the basename you want inside the directory (e.g. `Fi2010.zip`)
- **`download-url`** - original public URL of the file
- **`licence`** - short ID or full URL (e.g. `CC-BY-SA-4.0`)

Example - curate the full FI-2010 limit-order-book dataset (all 10 trading
days) from a Dropbox mirror:

```bash
scripts/curate-dataset.sh fi2010_all Fi2010.zip \
  "https://www.dropbox.com/s/6ywf3td7zdrp1n5/Fi2010.zip?dl=1" \
  CC-BY-SA-4.0
```

After the script finishes you will have the following structure ready to
commit or upload:

```
v1/fi2010_all/
 ├── Fi2010.zip          # ≈230 MB, contains day_1 … day_10
 ├── LICENSE.txt         # CC-BY-SA-4.0
 └── metadata.json       # size, sha256, provenance
```

You can now reference `v1/fi2010_all/Fi2010.zip` from tests or example code,
and downstream tooling can verify the checksum.

### Notes

- The script uses `curl -L --fail --retry 3`, so transient network hiccups are
  handled automatically.
- Re-running the script with the same arguments simply overwrites the existing
  files - useful when the upstream file is updated and you want to bump the
  checksum.
- Only basic validation is performed; ensure that the license you specify
  indeed permits redistribution.

---

For details on the other scripts, run them with `-h` or read the
inline comments; they are mostly invoked from CI and rarely need manual
execution.
