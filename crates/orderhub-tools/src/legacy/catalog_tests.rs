// SPDX-License-Identifier: LGPL-3.0-only

use std::{
    fs::{self, File},
    path::Path,
};

use nautilus_model::{
    data::QuoteTick,
    events::{
        AccountState, OrderFilled, account::stubs::cash_account_state_multi,
        order::stubs::order_filled,
    },
};
use parquet::arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder};
use rstest::rstest;
use rust_decimal_macros::dec;

fn read_zstd_page(frame: &[u8], uncompressed: usize) -> parquet::errors::Result<Vec<u8>> {
    use parquet::{
        basic::{Compression, Encoding, Repetition, Type as PhysicalType},
        column::page::{CompressedPage, Page, PageReader, PageWriter},
        file::{
            metadata::ColumnChunkMetaData,
            serialized_reader::SerializedPageReader,
            writer::{SerializedPageWriter, TrackedWrite},
        },
        schema::types::{SchemaDescriptor, Type},
    };
    use std::sync::Arc;
    let mut buffer = Vec::new();
    {
        let mut sink = TrackedWrite::new(&mut buffer);
        SerializedPageWriter::new(&mut sink).write_page(CompressedPage::new(
            Page::DataPage {
                buf: bytes::Bytes::copy_from_slice(frame),
                num_values: 1,
                encoding: Encoding::PLAIN,
                def_level_encoding: Encoding::RLE,
                rep_level_encoding: Encoding::RLE,
                statistics: None,
            },
            uncompressed,
        ))?;
    }
    let field = Arc::new(
        Type::primitive_type_builder("value", PhysicalType::BYTE_ARRAY)
            .with_repetition(Repetition::REQUIRED)
            .build()?,
    );
    let schema = SchemaDescriptor::new(Arc::new(
        Type::group_type_builder("schema")
            .with_fields(vec![field])
            .build()?,
    ));
    let metadata = ColumnChunkMetaData::builder(schema.column(0))
        .set_compression_codec(Compression::ZSTD(Default::default()).into())
        .set_data_page_offset(0)
        .set_num_values(1)
        .set_total_compressed_size(i64::try_from(buffer.len()).unwrap())
        .set_total_uncompressed_size(i64::try_from(uncompressed).unwrap())
        .build()?;
    let mut reader =
        SerializedPageReader::new(Arc::new(bytes::Bytes::from(buffer)), &metadata, 1, None)?;
    Ok(reader.get_next_page()?.unwrap().buffer().to_vec())
}

#[test]
fn pure_rust_zstd_reads_complete_frames_and_rejects_invalid_pages() {
    use parquet::{basic::Compression, file::properties::WriterProperties};
    let frame = [
        0x28, 0xb5, 0x2f, 0xfd, 0x20, 5, 41, 0, 0, b'h', b'e', b'l', b'l', b'o',
    ];
    assert_eq!(read_zstd_page(&frame, 5).unwrap(), b"hello");
    assert_eq!(
        read_zstd_page(&[frame.as_slice(), frame.as_slice()].concat(), 10).unwrap(),
        b"hellohello"
    );
    let mut checksummed = frame.to_vec();
    checksummed[4] = 0x24;
    checksummed.extend_from_slice(&[0xa3, 0x6d, 0x9f, 0x88]);
    assert_eq!(read_zstd_page(&checksummed, 5).unwrap(), b"hello");
    checksummed[13] ^= 1;
    for invalid in [
        &[][..],
        &frame[..13],
        &checksummed,
        &[0x28, 0xb5, 0x2f, 0xfd, 0xa0, 1, 0, 0, 4, 1, 0, 0],
    ] {
        assert!(read_zstd_page(invalid, 5).is_err());
    }
    for size in [4, 6, 64 * 1024 * 1024 + 1] {
        assert!(read_zstd_page(&frame, size).is_err());
    }
    let quote = QuoteTick::default();
    let batch = QuoteTick::encode_batch(&quote.metadata(), &[quote]).unwrap();
    let properties = WriterProperties::builder()
        .set_compression(Compression::ZSTD(Default::default()))
        .build();
    let mut writer = ArrowWriter::try_new(Vec::new(), batch.schema(), Some(properties)).unwrap();
    assert!(writer.write(&batch).and_then(|()| writer.flush()).is_err());
}

use super::{
    arrow::EncodeToRecordBatch,
    catalog::{migrate_parquet, verify_artifact},
};

fn write_fixture<T: EncodeToRecordBatch>(path: &Path, values: &[T]) {
    let batch = T::encode_batch(&values[0].metadata(), values).unwrap();
    let mut writer =
        ArrowWriter::try_new(File::create(path).unwrap(), batch.schema(), None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

#[rstest]
fn filled_events_preserve_quantity_money_and_notional(mut order_filled: OrderFilled) {
    order_filled.last_qty = "0.561".parse().unwrap();
    order_filled.last_px = "22000".parse().unwrap();
    order_filled.commission = Some("12.20 USDT".parse().unwrap());
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("fills.parquet");
    let destination = directory.path().join("fills.v1.jsonl");
    write_fixture(&source, &[order_filled.clone(), order_filled]);
    let original = fs::read(&source).unwrap();
    let summary = migrate_parquet(&source, &destination, "OrderFilled").unwrap();
    assert_eq!(summary.rows, 2);
    assert_eq!(summary.events["OrderFilled"], 2);
    assert_eq!(summary.exact_totals["last_qty"], dec!(1.122));
    assert_eq!(summary.exact_totals["commission.USDT"], dec!(24.40));
    assert_eq!(summary.exact_totals["executed_notional.USDT"], dec!(24684));
    assert_eq!(verify_artifact(&destination).unwrap(), summary);
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(migrate_parquet(&source, &destination, "OrderFilled").is_err());
}

#[test]
fn account_balances_retain_currency_totals() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("account.parquet");
    let destination = directory.path().join("account.v1.jsonl");
    write_fixture::<AccountState>(&source, &[cash_account_state_multi()]);
    let summary = migrate_parquet(&source, &destination, "AccountState").unwrap();
    assert_eq!(summary.events["AccountState"], 1);
    assert_eq!(summary.exact_totals["balances.total.BTC"], dec!(10));
    assert_eq!(verify_artifact(&destination).unwrap(), summary);
}

#[test]
fn retained_64_and_128_bit_historical_files_are_readable() {
    let directory = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/nautilus/legacy");
    let mut standard = std::collections::BTreeMap::new();
    for precision in ["64-bit", "128-bit"] {
        for (name, record_type) in [
            ("quotes", "QuoteTick"),
            ("trades", "TradeTick"),
            ("bars", "Bar"),
            ("deltas", "OrderBookDelta"),
            ("depths", "OrderBookDepth"),
        ] {
            let source = fixtures.join(precision).join(format!("{name}.parquet"));
            let rows = ParquetRecordBatchReaderBuilder::try_new(File::open(&source).unwrap())
                .unwrap()
                .metadata()
                .file_metadata()
                .num_rows();
            let destination = directory
                .path()
                .join(format!("{precision}-{name}.v1.jsonl"));
            let summary = migrate_parquet(&source, &destination, record_type)
                .unwrap_or_else(|e| panic!("{precision}/{name}: {e:#}"));
            assert_eq!(summary.rows, u64::try_from(rows).unwrap());
            assert!(summary.rows > 0);
            assert_eq!(verify_artifact(&destination).unwrap(), summary);
            if precision == "64-bit" {
                standard.insert(name, summary);
            } else {
                assert_eq!(summary, standard[name], "precision changed {name} facts");
            }
        }
    }
}

#[test]
fn modified_incomplete_and_future_artifacts_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("quote.parquet");
    let destination = directory.path().join("quote.v1.jsonl");
    write_fixture(&source, &[QuoteTick::default()]);
    migrate_parquet(&source, &destination, "QuoteTick").unwrap();
    let original = fs::read_to_string(&destination).unwrap();
    let changed = original.replace("\"sequence\":1", "\"sequence\":2");
    assert_ne!(changed, original);
    fs::write(&destination, changed).unwrap();
    assert!(verify_artifact(&destination).is_err());
    fs::write(
        &destination,
        original.replace("\"format_version\":1", "\"format_version\":99"),
    )
    .unwrap();
    assert!(verify_artifact(&destination).is_err());
    fs::write(&destination, original.lines().next().unwrap()).unwrap();
    assert!(verify_artifact(&destination).is_err());
    assert!(
        migrate_parquet(
            &source,
            &directory.path().join("unknown.jsonl"),
            "UnknownRecord"
        )
        .is_err()
    );
    assert!(!directory.path().join("unknown.jsonl").exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn missing_source_and_missing_parent_do_not_create_output() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.parquet");
    let destination = directory.path().join("output.jsonl");
    assert!(migrate_parquet(&missing, &destination, "QuoteTick").is_err());
    assert!(!destination.exists());
    let source = directory.path().join("quote.parquet");
    write_fixture(&source, &[QuoteTick::default()]);
    assert!(
        migrate_parquet(
            &source,
            &directory.path().join("missing/output.jsonl"),
            "QuoteTick"
        )
        .is_err()
    );
    assert!(!directory.path().join("missing").exists());
}
