// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under
//  the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
//  KIND, either express or implied. See the License for the specific language governing
//  permissions and limitations under the License.
// -------------------------------------------------------------------------------------------------

use orderhub_gateway::journal::{AdmissionRecord, BusinessJournal, RecoveryIdentity};
use orderhub_gateway::ledger::{BusinessOrder, ExecutionRecord, RiskContext};
use rust_decimal::Decimal;

pub(crate) fn directory() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../audit/orderhub-simplification-2026-10-02/run-16/runtime-tmp");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

pub(crate) fn identity() -> RecoveryIdentity {
    RecoveryIdentity {
        instance_id: "p6-isolated-test".into(),
        binary_hash: "test-binary".into(),
        configuration_hash: "test-profile".into(),
    }
}

pub(crate) fn record() -> AdmissionRecord {
    AdmissionRecord {
        journal_epoch: 0,
        seq: 0,
        submitter_id: "trader-1".into(),
        request_id: "p6-order".into(),
        request_kind: "SubmitOrder".into(),
        request_fingerprint: "p6-order-buy-2-150".into(),
        strategy_id: "ORDERHUB-S001".into(),
        instrument_id: "AAPL.GOX".into(),
        order_side: "BUY".into(),
        quantity: "2".into(),
        price: "150.00".into(),
        time_in_force: "DAY".into(),
        post_only: false,
        reduce_only: false,
        submit_before_ns: None,
        ts_init_ns: 1_800_000_000_000_000_000,
    }
}

pub(crate) fn context() -> RiskContext {
    RiskContext {
        account_id: "GOX-001".into(),
        strategy_budget: Decimal::from(1_000_000),
        account_budget: Decimal::from(1_000_000),
        owned_quantity: Decimal::from(20),
        owned_cost: Decimal::from(3_000),
        account_quantity: Decimal::from(25),
        strategy_exposure_limit: Decimal::from(1_000_000),
        account_exposure_limit: Decimal::from(2_000_000),
        reference_price: Decimal::from(150),
        fee_buffer: Decimal::ONE,
        maximum_quantity: Decimal::from(10_000),
        maximum_notional: Decimal::from(1_000_000),
        risk_config_version: "p6-test".into(),
    }
}

pub(crate) fn admit(journal: &BusinessJournal) -> BusinessOrder {
    let admission = journal
        .admit_business(record(), Some(context()), None)
        .unwrap();
    journal.order(admission.seq).unwrap().unwrap()
}

pub(crate) fn fill(order: &BusinessOrder) -> ExecutionRecord {
    ExecutionRecord {
        native_event: None,
        event_id: "p6-fill".into(),
        trade_id: Some("p6-trade".into()),
        client_order_id: order.client_order_id.clone(),
        account_id: order.account_id.clone(),
        strategy_id: order.admission.strategy_id.clone(),
        instrument_id: order.admission.instrument_id.clone(),
        kind: "FILL".into(),
        quantity: "2".into(),
        price: "150".into(),
        fee: "0.50".into(),
        currency: "USD".into(),
        ts_event_ns: 1_800_000_000_000_000_010,
        ts_received_ns: 1_800_000_000_000_000_011,
        reason: String::new(),
    }
}
