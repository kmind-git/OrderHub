# Convert historical Parquet files

This OrderHub checkout retains offline historical conversion in
`orderhub-tools/legacy-catalog`, exposed by the `orderhub-migrate` binary.
The general catalog CLI and `nautilus_persistence` migration API are retired.
OrderHub's live authority is its versioned redb business journal.

The converter reads one local Parquet file and writes a separate version 1 JSONL artifact.
It does not migrate a catalog directory, write a new Parquet catalog, accept object-store URIs,
or import records into the business journal.

## Convert a selected original file

Choose a supported record type and an existing destination directory. The output file must
not exist. For example, run from the repository root:

```bash
cargo run --locked --profile nextest -p orderhub-tools \
  --no-default-features --features legacy-catalog --bin orderhub-migrate -- \
  parquet --source old.parquet --destination converted.v1.jsonl --record-type QuoteTick
```

The tool preserves the source file. It verifies a temporary output and rechecks the source
digest before publishing the destination without overwriting an existing file.
The destination filesystem must support hard links. Failure is reported explicitly.

The retained original-file compatibility tests cover 64-bit and 128-bit historical market-data
layouts. Add `high-precision` to the feature list when that domain configuration is required:

```bash
cargo run --locked --profile nextest -p orderhub-tools \
  --no-default-features --features legacy-catalog,high-precision --bin orderhub-migrate -- \
  parquet --source old.parquet --destination converted.v1.jsonl --record-type QuoteTick
```

## Verify the artifact

```bash
cargo run --locked --profile nextest -p orderhub-tools \
  --no-default-features --features legacy-catalog --bin orderhub-migrate -- \
  verify --artifact converted.v1.jsonl
```

Verification checks the version, record sequence, native decoded values, record digest,
counts, and quantity or money totals. The artifact contains the original file's BLAKE3 digest.
Keep the original file and compare its digest when proving the artifact's source; internal
consistency does not authenticate an external trading fact.

## Supported scope and limits

- Built-in market data, accounts, order and position events, reports, snapshots, and instruments
  are supported where their record type and schema are recognized.
- Arbitrary CustomData and unknown schemas are rejected rather than inferred.
- A source file is limited to 128 MiB, output to 512 MiB, records to one million, and each JSONL
  line to 1 MiB.
- Historical Zstandard decoding uses the retained pure Rust Parquet patch. Its decoding window
  and page limits are 64 MiB; Zstandard writing is rejected.
- Conversion does not open live trading or restore the node to READY.

See [framework retirement](../orderhub/framework-retirement.md#离线历史迁移) for the format,
source provenance, compression boundary, and original-file compatibility requirements.
Recovering an OrderHub business journal instead uses the separate
[recovery tool](../orderhub/recovery-runtime.md#运维入口).
