// SPDX-License-Identifier: LGPL-3.0-only

//! Offline Parquet to versioned native JSON conversion with integrity checks.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
    str::FromStr,
};

use anyhow::{Context, Result, ensure};
use arrow::record_batch::RecordBatch;
use nautilus_model::{data::*, events::*, instruments::InstrumentAny, reports::*};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::arrow::{
    DecodeTypedFromRecordBatch,
    legacy::{
        LegacyTranscodeState, normalize_legacy_fixed_columns,
        transcode_legacy_record_batch_with_state,
    },
};

const FORMAT_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ROWS: u64 = 1_000_000;
const MAX_LINE_BYTES: usize = 1024 * 1024;

macro_rules! catalog_types {
    ($callback:ident) => {
        $callback!(
            QuoteTick,
            TradeTick,
            Bar,
            OrderBookDelta,
            OrderBookDepth,
            MarkPriceUpdate,
            IndexPriceUpdate,
            FundingRateUpdate,
            InstrumentStatus,
            InstrumentClose,
            OptionGreeks,
            AccountState,
            OrderInitialized,
            OrderDenied,
            OrderEmulated,
            OrderSubmitted,
            OrderAccepted,
            OrderRejected,
            OrderPendingCancel,
            OrderCanceled,
            OrderCancelRejected,
            OrderExpired,
            OrderTriggered,
            OrderPendingUpdate,
            OrderReleased,
            OrderModifyRejected,
            OrderUpdated,
            OrderFilled,
            OrderFillVoided,
            PositionOpened,
            PositionChanged,
            PositionClosed,
            PositionAdjusted,
            OrderSnapshot,
            PositionSnapshot,
            FillReport,
            OrderStatusReport,
            PositionStatusReport,
            ExecutionMassStatus,
        )
    };
}

fn validate_record_type(record_type: &str) -> Result<()> {
    macro_rules! supported {
        ($($ty:ident),+ $(,)?) => {
            ensure!([$(stringify!($ty),)+ "InstrumentAny"].contains(&record_type), "Unsupported historical record type: {record_type}")
        };
    }
    catalog_types!(supported);
    Ok(())
}

fn validate_native(record_type: &str, value: &Value) -> Result<()> {
    fn validate<T: DeserializeOwned + Serialize>(value: &Value) -> Result<()> {
        let record: T = serde_json::from_value(value.clone())?;
        ensure!(
            serde_json::to_value(record)? == *value,
            "Catalog record is not canonical native data"
        );
        Ok(())
    }
    macro_rules! types {
        ($($ty:ident),+ $(,)?) => {
            match record_type {
                $(stringify!($ty) => validate::<$ty>(value),)+
                "InstrumentAny" => validate::<InstrumentAny>(value),
                _ => anyhow::bail!("Unsupported historical record type: {record_type}"),
            }
        };
    }
    catalog_types!(types)
}

/// Facts compared before publishing a converted catalog artifact.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSummary {
    pub rows: u64,
    pub events: BTreeMap<String, u64>,
    pub exact_totals: BTreeMap<String, Decimal>,
    pub records_blake3: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format_version: u32,
    record_type: String,
    source_blake3: String,
}

fn add_total(totals: &mut BTreeMap<String, Decimal>, key: String, amount: Decimal) -> Result<()> {
    let old = totals.get(&key).copied().unwrap_or_default();
    totals.insert(
        key,
        old.checked_add(amount).context("Catalog total overflow")?,
    );
    Ok(())
}

fn collect_totals(
    value: &Value,
    prefix: &str,
    totals: &mut BTreeMap<String, Decimal>,
) -> Result<()> {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                if let Some(text) = value.as_str() {
                    if matches!(
                        name.as_str(),
                        "quantity"
                            | "last_qty"
                            | "bid_size"
                            | "ask_size"
                            | "size"
                            | "volume"
                            | "price"
                            | "last_px"
                            | "bid_price"
                            | "ask_price"
                            | "open"
                            | "high"
                            | "low"
                            | "close"
                    ) {
                        add_total(
                            totals,
                            key.clone(),
                            Decimal::from_str(text)
                                .with_context(|| format!("Invalid exact field {key}"))?,
                        )?;
                    } else if matches!(
                        name.as_str(),
                        "commission"
                            | "total"
                            | "free"
                            | "locked"
                            | "realized_pnl"
                            | "unrealized_pnl"
                    ) {
                        let (amount, currency) = text
                            .rsplit_once(' ')
                            .context("Money field lacks currency")?;
                        add_total(
                            totals,
                            format!("{key}.{currency}"),
                            Decimal::from_str(amount)?,
                        )?;
                    }
                }
                collect_totals(value, &key, totals)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_totals(value, prefix, totals)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn record_summary(summary: &mut CatalogSummary, record_type: &str, value: &Value) -> Result<()> {
    summary.rows += 1;
    ensure!(
        summary.rows <= MAX_ROWS,
        "Catalog exceeds the offline row limit"
    );
    if record_type.starts_with("Order")
        || record_type.starts_with("Position")
        || record_type == "AccountState"
    {
        *summary.events.entry(record_type.to_string()).or_default() += 1;
    }
    collect_totals(value, "", &mut summary.exact_totals)?;
    if record_type == "OrderFilled" {
        let quantity = Decimal::from_str(
            value["last_qty"]
                .as_str()
                .context("Missing fill quantity")?,
        )?;
        let price = Decimal::from_str(value["last_px"].as_str().context("Missing fill price")?)?;
        let notional = quantity
            .checked_mul(price)
            .context("Fill notional overflow")?;
        let currency = value["currency"]
            .as_str()
            .context("Missing fill currency")?;
        add_total(
            &mut summary.exact_totals,
            format!("executed_notional.{currency}"),
            notional,
        )?;
    }
    Ok(())
}

fn typed_values<T>(batch: RecordBatch) -> Result<Vec<Value>>
where
    T: DecodeTypedFromRecordBatch + Serialize + DeserializeOwned,
{
    let metadata = batch.schema().metadata().clone();
    T::decode_typed_batch(&metadata, batch)?
        .into_iter()
        .map(|record| {
            let value = serde_json::to_value(&record)?;
            let restored: T = serde_json::from_value(value.clone())?;
            ensure!(
                serde_json::to_value(restored)? == value,
                "Native serialization changed a record"
            );
            Ok(value)
        })
        .collect()
}

fn decode(record_type: &str, batch: RecordBatch) -> Result<Vec<Value>> {
    if record_type == "OrderBookDepth" && batch.column_by_name("bid_price_0").is_some() {
        return super::flat_depth::decode(&batch)?
            .into_iter()
            .map(|record| Ok(serde_json::to_value(record)?))
            .collect();
    }
    if record_type == "InstrumentAny" {
        return super::arrow::instrument::decode_instrument_any_batch(
            batch.schema().metadata(),
            &batch,
        )?
        .into_iter()
        .map(|record| Ok(serde_json::to_value(record)?))
        .collect();
    }
    macro_rules! types {
        ($($ty:ident),+ $(,)?) => {
            match record_type {
                $(stringify!($ty) => typed_values::<$ty>(batch),)+
                _ => anyhow::bail!("Unsupported historical record type: {record_type}"),
            }
        };
    }
    catalog_types!(types)
}

fn read_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let count = reader
        .take((MAX_LINE_BYTES + 1) as u64)
        .read_until(b'\n', &mut line)?;
    ensure!(
        line.len() <= MAX_LINE_BYTES,
        "Catalog line exceeds the limit"
    );
    if count == 0 {
        return Ok(None);
    }
    ensure!(line.last() == Some(&b'\n'), "Catalog line is incomplete");
    line.pop();
    Ok(Some(line))
}

fn write_json_line(
    writer: &mut impl Write,
    value: &impl Serialize,
    written_bytes: &mut u64,
) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    ensure!(
        line.len() <= MAX_LINE_BYTES,
        "Catalog line exceeds the limit"
    );
    let total = written_bytes
        .checked_add(line.len() as u64)
        .context("Catalog artifact size overflow")?;
    ensure!(
        total <= MAX_ARTIFACT_BYTES,
        "Catalog artifact exceeds the limit"
    );
    writer.write_all(&line)?;
    *written_bytes = total;
    Ok(())
}

/// Verifies the version, row count, exact economic totals, events, and record digest.
///
/// # Errors
///
/// Returns an error for an unsupported, oversized, incomplete, or modified artifact.
pub fn verify_artifact(path: &Path) -> Result<CatalogSummary> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_ARTIFACT_BYTES,
        "Catalog artifact exceeds the limit"
    );
    let mut reader = BufReader::new(file);
    let header: Header =
        serde_json::from_slice(&read_line(&mut reader)?.context("Missing catalog header")?)?;
    ensure!(
        header.format_version == FORMAT_VERSION,
        "Unsupported catalog format version"
    );
    ensure!(
        header.source_blake3.len() == 64,
        "Invalid catalog source digest"
    );
    blake3::Hash::from_hex(&header.source_blake3).context("Invalid catalog source digest")?;
    validate_record_type(&header.record_type)?;
    let mut summary = CatalogSummary::default();
    let mut digest = blake3::Hasher::new();
    while let Some(line) = read_line(&mut reader)? {
        let value: Value = serde_json::from_slice(&line)?;
        if let Some(footer) = value.get("summary") {
            ensure!(
                value.as_object().is_some_and(|fields| fields.len() == 1),
                "Unknown catalog footer fields"
            );
            let expected: CatalogSummary = serde_json::from_value(footer.clone())?;
            summary.records_blake3 = digest.finalize().to_hex().to_string();
            ensure!(summary == expected, "Catalog compatibility proof mismatch");
            ensure!(read_line(&mut reader)?.is_none(), "Trailing catalog data");
            return Ok(summary);
        }
        let data = value.get("data").context("Missing catalog row data")?;
        ensure!(
            value.as_object().is_some_and(|row| row.len() == 2),
            "Unknown catalog row fields"
        );
        validate_native(&header.record_type, data)?;
        ensure!(
            value.get("sequence").and_then(Value::as_u64) == Some(summary.rows + 1),
            "Catalog sequence mismatch"
        );
        record_summary(&mut summary, &header.record_type, data)?;
        digest.update(&serde_json::to_vec(data)?);
        digest.update(b"\n");
    }
    anyhow::bail!("Missing catalog compatibility proof")
}

/// Converts one immutable Parquet snapshot into a separately published JSON artifact.
///
/// Source files are never modified. An existing destination is never overwritten.
/// Financial values retain native decimal strings; no research query engine is used.
///
/// # Errors
///
/// Returns an error for unsupported data, limits, conversion, verification, or publication failure.
pub fn migrate_parquet(
    source: &Path,
    destination: &Path,
    record_type: &str,
) -> Result<CatalogSummary> {
    validate_record_type(record_type)?;
    ensure!(source.is_file(), "Catalog source must be an existing file");
    ensure!(!destination.exists(), "Catalog destination already exists");
    let source_file = File::open(source)?;
    ensure!(
        source_file.metadata()?.len() <= MAX_SOURCE_BYTES,
        "Catalog source exceeds the limit"
    );
    let mut bytes = Vec::new();
    source_file
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_SOURCE_BYTES,
        "Catalog source grew beyond the limit"
    );
    let source_digest = blake3::hash(&bytes).to_hex().to_string();
    let builder = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(bytes))?;
    let source_metadata = builder.schema().metadata().clone();
    let reader = builder.with_batch_size(4096).build()?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ensure!(parent.is_dir(), "Catalog destination parent must exist");
    let staging = parent.join(format!(
        ".orderhub-migrate-{}.pending",
        nautilus_core::UUID4::new()
    ));
    let result = (|| -> Result<CatalogSummary> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)?;
        let mut writer = BufWriter::new(file);
        let header = Header {
            format_version: FORMAT_VERSION,
            record_type: record_type.to_string(),
            source_blake3: source_digest.clone(),
        };
        let mut written_bytes = 0;
        write_json_line(&mut writer, &header, &mut written_bytes)?;
        let mut summary = CatalogSummary::default();
        let mut digest = blake3::Hasher::new();
        let mut transcode = LegacyTranscodeState::default();
        for batch in reader {
            let batch = batch?;
            // Parquet's reader batches omit file-level schema metadata.
            // Preserve the metadata used to interpret fixed-precision market records.
            let schema = std::sync::Arc::new(arrow::datatypes::Schema::new_with_metadata(
                batch.schema().fields().clone(),
                source_metadata.clone(),
            ));
            let batch = batch.with_schema(schema)?;
            let batch = normalize_legacy_fixed_columns(&batch)?;
            let converted = transcode_legacy_record_batch_with_state(
                record_type,
                &source.display().to_string(),
                batch,
                &mut transcode,
            )?;
            for batch in converted.batches {
                for value in decode(record_type, batch)? {
                    record_summary(&mut summary, record_type, &value)?;
                    digest.update(&serde_json::to_vec(&value)?);
                    digest.update(b"\n");
                    write_json_line(
                        &mut writer,
                        &serde_json::json!({"sequence": summary.rows, "data": value}),
                        &mut written_bytes,
                    )?;
                }
            }
        }
        summary.records_blake3 = digest.finalize().to_hex().to_string();
        write_json_line(
            &mut writer,
            &serde_json::json!({"summary": summary}),
            &mut written_bytes,
        )?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        ensure!(
            verify_artifact(&staging)? == summary,
            "Catalog readback changed native facts"
        );
        let mut current = Vec::new();
        File::open(source)?
            .take(MAX_SOURCE_BYTES + 1)
            .read_to_end(&mut current)?;
        ensure!(
            blake3::hash(&current).to_hex().as_str() == source_digest,
            "Catalog source changed during migration"
        );
        fs::hard_link(&staging, destination)
            .context("Cannot publish catalog without overwriting")?;
        Ok(summary)
    })();
    let _ = fs::remove_file(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::{MAX_ARTIFACT_BYTES, MAX_LINE_BYTES, write_json_line};

    #[test]
    fn artifact_limit_includes_newline_and_rejects_before_writing() {
        let mut output = Vec::new();
        let mut written = MAX_ARTIFACT_BYTES - 5;
        assert!(write_json_line(&mut output, &"abc", &mut written).is_err());
        assert!(output.is_empty());
        assert_eq!(written, MAX_ARTIFACT_BYTES - 5);

        written = MAX_ARTIFACT_BYTES - 6;
        write_json_line(&mut output, &"abc", &mut written).unwrap();
        assert_eq!(output, b"\"abc\"\n");
        assert_eq!(written, MAX_ARTIFACT_BYTES);
        assert!(write_json_line(&mut output, &"", &mut written).is_err());
        assert_eq!(output, b"\"abc\"\n");
    }

    #[test]
    fn oversized_line_is_rejected_before_writing() {
        let mut output = Vec::new();
        let mut written = 0;
        assert!(
            write_json_line(&mut output, &"x".repeat(MAX_LINE_BYTES - 2), &mut written).is_err()
        );
        assert!(output.is_empty());
        assert_eq!(written, 0);

        write_json_line(&mut output, &"x".repeat(MAX_LINE_BYTES - 3), &mut written).unwrap();
        assert_eq!(output.len(), MAX_LINE_BYTES);
        assert_eq!(written, MAX_LINE_BYTES as u64);
    }
}
