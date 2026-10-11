// SPDX-License-Identifier: LGPL-3.0-only

//! Strict adapter for the historical ten-level, flat-column depth layout.

use anyhow::{Context, Result, ensure};
use arrow::{
    array::{Array, Decimal128Array, UInt8Array, UInt32Array, UInt64Array},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{BookOrder, OrderBookDepth},
    enums::OrderSide,
};

use super::arrow::{
    decode_required_decimal_price, decode_required_decimal_quantity, decode_required_timestamp,
    decode_required_u8, decode_required_u64, parse_metadata, record_batch_with_u64_timestamps,
};

fn column<'a, T: Array + 'static>(batch: &'a RecordBatch, name: &str) -> Result<&'a T> {
    let value = batch
        .column_by_name(name)
        .with_context(|| format!("Missing historical depth field {name}"))?;
    value.as_any().downcast_ref::<T>().with_context(|| {
        format!(
            "Invalid historical depth field {name}: {:?}",
            value.data_type()
        )
    })
}

pub(super) fn decode(batch: &RecordBatch) -> Result<Vec<OrderBookDepth>> {
    ensure!(
        batch.num_columns() == 64,
        "Unknown historical flat depth layout"
    );
    let (instrument, price_precision, size_precision) = parse_metadata(batch.schema().metadata())?;
    let batch = record_batch_with_u64_timestamps(batch)?;
    let mut result = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        let mut sides = Vec::with_capacity(2);
        for (prefix, side) in [("bid", OrderSide::Buy), ("ask", OrderSide::Sell)] {
            let mut orders = Vec::new();
            let mut counts = Vec::new();
            let mut padding = false;
            for level in 0..10 {
                let prices = column::<Decimal128Array>(&batch, &format!("{prefix}_price_{level}"))?;
                let sizes = column::<Decimal128Array>(&batch, &format!("{prefix}_size_{level}"))?;
                let count = column::<UInt32Array>(&batch, &format!("{prefix}_count_{level}"))?;
                ensure!(!count.is_null(row), "Null historical depth count");
                let price = decode_required_decimal_price(prices, price_precision, "price", row)?;
                let size = decode_required_decimal_quantity(sizes, size_precision, "size", row)?;
                let count = count.value(row);
                if size.is_zero() {
                    ensure!(
                        price.is_zero() && count == 0,
                        "Nonempty historical depth padding"
                    );
                    padding = true;
                    continue;
                }
                ensure!(
                    !padding,
                    "Historical depth contains an interior empty level"
                );
                orders.push(BookOrder::new(side, price, size, 0));
                counts.push(count);
            }
            sides.push((orders, counts));
        }
        let (asks, ask_counts) = sides.pop().unwrap();
        let (bids, bid_counts) = sides.pop().unwrap();
        result.push(OrderBookDepth::new_checked(
            instrument,
            bids,
            asks,
            bid_counts,
            ask_counts,
            decode_required_u8(column::<UInt8Array>(&batch, "flags")?, "flags", row)?,
            decode_required_u64(column::<UInt64Array>(&batch, "sequence")?, "sequence", row)?,
            decode_required_timestamp(column::<UInt64Array>(&batch, "ts_event")?, "ts_event", row)?,
            decode_required_timestamp(column::<UInt64Array>(&batch, "ts_init")?, "ts_init", row)?,
        )?);
    }
    Ok(result)
}
