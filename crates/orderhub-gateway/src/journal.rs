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

//! Append-only local business journal backed by `redb`.
//!
//! This is the M0 prototype of the OrderHub business log described in the
//! recovery contract. It owns a single `redb` database file with:
//!
//! - `JOURNAL`: sequence-numbered entries (admissions and outcomes), inserted
//!   in commit order.
//! - `REQUEST_INDEX`: idempotency keys mapped to their admission sequence.
//! - `META`: journal epoch and last committed sequence.
//!
//! Every [`BusinessJournal::append_admission`] and
//! [`BusinessJournal::append_outcome`] commits with
//! [`redb::Durability::Immediate`], so a committed record survives process and
//! host crash under the storage device's fsync semantics (see the recovery
//! contract for the distinction from media-failure durability).
//!
//! Sequence allocation happens inside the same transaction as the record
//! insert, so a crash can never re-issue a committed sequence number.
//! `client_order_id` values derived from `(epoch, seq)` are therefore stable
//! across restarts and never reused, as required by the admission contract.

use std::fmt::Debug;
use std::path::Path;

use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

/// Journal entries in commit order, keyed by monotonically increasing `seq`.
const JOURNAL: TableDefinition<u64, &[u8]> = TableDefinition::new("orderhub_journal");
/// Idempotency request key (bytes) -> admission `seq`.
const REQUEST_INDEX: TableDefinition<&[u8], u64> = TableDefinition::new("orderhub_request_index");
/// Small metadata table: key `1` = journal epoch, key `2` = last committed seq.
const META: TableDefinition<u64, u64> = TableDefinition::new("orderhub_meta");

const META_EPOCH: u64 = 1;
const META_LAST_SEQ: u64 = 2;
const META_SCHEMA: u64 = 3;
const META_BYTES: u64 = 4;
const ADMISSION_BYTES: u64 = 96 * 1024 * 1024;
const MAXIMUM_BYTES: u64 = 128 * 1024 * 1024;
const MAXIMUM_RECORDS: u64 = 1_000_000;
const CONTROL: TableDefinition<u64, &[u8]> = TableDefinition::new("orderhub_control_v1");
const ORDERS: TableDefinition<u64, &[u8]> = TableDefinition::new("orderhub_orders_v1");
const RESULTS: TableDefinition<u64, &[u8]> = TableDefinition::new("orderhub_results_v1");
const EXECUTIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("orderhub_executions_v1");
const BALANCES: TableDefinition<&str, &[u8]> = TableDefinition::new("orderhub_balances_v1");
const INPUTS: TableDefinition<&str, &[u8]> = TableDefinition::new("orderhub_inputs_v1");
const OUTCOMES: TableDefinition<u64, &[u8]> = TableDefinition::new("orderhub_outcomes_v1");
const SECRETS: TableDefinition<&str, &[u8]> = TableDefinition::new("orderhub_secrets_v1");
const EVENT_VIEWS: TableDefinition<u64, &[u8]> = TableDefinition::new("orderhub_event_views_v1");

use crate::ledger::{
    BusinessOrder, ExecutionRecord, RequestResult, RiskBalance, RiskContext, add, decimal, invalid,
    mul, sub,
};

#[path = "recovery_storage.rs"]
mod recovery_storage;

use recovery_storage::{Mutation, RECOVERY, RECOVERY_COMMITS, RowKey};
pub use recovery_storage::{RecoveryIdentity, RecoveryManifest, RecoveryReport};

/// Errors raised by the business journal.
#[derive(Debug, Clone)]
pub enum JournalError {
    /// The durable commit may have succeeded; all risk occupancy must be retained.
    CommitUnknown(String),
    /// An underlying storage error from `redb`.
    Storage(String),
    /// A record could not be encoded or decoded.
    Encoding(String),
}

impl std::error::Error for JournalError {}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommitUnknown(e) => write!(f, "journal commit result unknown: {e}"),
            Self::Storage(err) => write!(f, "journal storage error: {err}"),
            Self::Encoding(err) => write!(f, "journal encoding error: {err}"),
        }
    }
}

impl From<redb::Error> for JournalError {
    fn from(err: redb::Error) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<redb::TableError> for JournalError {
    fn from(err: redb::TableError) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<redb::CommitError> for JournalError {
    fn from(err: redb::CommitError) -> Self {
        Self::CommitUnknown(err.to_string())
    }
}

impl From<redb::TransactionError> for JournalError {
    fn from(err: redb::TransactionError) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<redb::StorageError> for JournalError {
    fn from(err: redb::StorageError) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<redb::DatabaseError> for JournalError {
    fn from(err: redb::DatabaseError) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<redb::SetDurabilityError> for JournalError {
    fn from(err: redb::SetDurabilityError) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<serde_json::Error> for JournalError {
    fn from(err: serde_json::Error) -> Self {
        Self::Encoding(err.to_string())
    }
}

/// A durably admitted order request.
///
/// Monetary and quantity fields are decimal strings; float/double are
/// forbidden by the admission contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionRecord {
    /// Journal epoch at admission time.
    pub journal_epoch: u64,
    /// Committed sequence number assigned by the journal.
    pub seq: u64,
    /// Server-side submitter identity (from credential mapping in M2+).
    pub submitter_id: String,
    /// Client-generated unique request ID, stable across retries.
    pub request_id: String,
    /// RPC method of the request (idempotency namespace).
    pub request_kind: String,
    /// Canonical digest of the normalized business fields.
    pub request_fingerprint: String,
    /// Owning strategy for attribution, limits, and PnL.
    pub strategy_id: String,
    pub instrument_id: String,
    pub order_side: String,
    pub quantity: String,
    pub price: String,
    pub time_in_force: String,
    pub post_only: bool,
    pub reduce_only: bool,
    /// Business deadline for first dispatch (ns since epoch), if any.
    pub submit_before_ns: Option<u64>,
    /// Gateway receive time (ns since epoch).
    pub ts_init_ns: u64,
}

/// The durable result of an admitted request or a later lifecycle fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalOutcome {
    /// A complete, attributed execution fact with stable economic identity.
    Execution(Box<ExecutionRecord>),
    /// The request was durably admitted and an order was created.
    Admitted {
        /// Order ID derived deterministically from `(journal_epoch, seq)`.
        client_order_id: String,
    },
    /// A definitive business rejection (risk rules, instrument, deadline).
    Denied {
        /// Stable reason string from the denying component.
        reason: String,
    },
    /// The order crossed the dispatch boundary (conservative uncertainty edge).
    DispatchStarted { client_order_id: String },
    /// A fill (or partial fill) was observed from the execution chain.
    Filled {
        client_order_id: String,
        filled_qty: String,
    },
    /// The order reached a cancelled terminal state.
    Canceled { client_order_id: String },
}

/// One journal entry in commit order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalEntry {
    /// Persisted readiness and execution connection transition.
    Control(crate::ledger::ControlRecord),
    /// Validated input fact and its local receipt time, committed before query visibility.
    Input {
        event: Box<crate::inputs::InputEvent>,
        received_at_ns: u64,
    },
    /// An admitted request (idempotency anchor).
    Admission(Box<AdmissionRecord>),
    /// A later outcome appended for a previously admitted request.
    Outcome {
        /// The admission `seq` this outcome applies to.
        origin_seq: u64,
        outcome: JournalOutcome,
    },
}

/// The result of an idempotency lookup.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    /// The original admission.
    pub admission: AdmissionRecord,
    /// The most recent committed outcome for this request, if any.
    pub outcome: Option<JournalOutcome>,
    /// Original immutable RPC result, never replaced by a fill or cancellation.
    pub result: RequestResult,
}

/// A pending durable write for group commit.
#[derive(Debug, Clone)]
pub enum PendingEntry {
    /// Persisted readiness and execution connection transition.
    Control(crate::ledger::ControlRecord),
    /// Validated market/account input to commit and project.
    Input {
        event: Box<crate::inputs::InputEvent>,
        received_at_ns: u64,
    },
    /// Atomic request, order intent, reservation, and initial result.
    BusinessAdmission {
        record: Box<AdmissionRecord>,
        context: Option<Box<RiskContext>>,
        rejection: Option<String>,
    },
    /// An admission to append and index.
    Admission(Box<AdmissionRecord>),
    /// An outcome to append for an existing admission.
    Outcome {
        origin_seq: u64,
        outcome: JournalOutcome,
    },
}

/// The committed result of one pending entry.
#[derive(Debug, Clone)]
pub enum CommittedEntry {
    /// Persisted readiness transition.
    Control { seq: u64 },
    /// Committed input fact.
    Input { seq: u64 },
    /// The admission with its journal-assigned sequence.
    Admission(Box<AdmissionRecord>),
    /// An outcome record with its own sequence and its origin.
    Outcome { seq: u64, origin_seq: u64 },
}

/// Append-only business journal with immediate-durability commits.
#[derive(Debug)]
pub struct BusinessJournal {
    db: Database,
    journal_epoch: u64,
}

/// A committed dataset captured at one business cursor.
#[derive(Debug, Clone)]
pub struct CommittedSnapshot {
    pub control: Option<crate::ledger::ControlRecord>,
    pub cursor: u64,
    pub orders: Vec<BusinessOrder>,
    pub inputs: Vec<crate::inputs::InputEvent>,
    pub positions: Vec<orderhub_proto::orderhub::v1::PositionView>,
    pub accounts: Vec<orderhub_proto::orderhub::v1::AccountView>,
}

impl BusinessJournal {
    /// Opens schema v1 storage; unversioned historical databases need offline migration.
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let db = if path.exists() {
            Database::open(path)?
        } else {
            Database::builder().create(path)?
        };
        Self::from_database(db)
    }

    /// Initializes a redb database selected by the caller's storage backend policy.
    ///
    /// # Errors
    ///
    /// Returns storage, schema, or metadata validation errors.
    pub fn from_database(db: Database) -> Result<Self, JournalError> {
        let mut write = db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        let epoch;
        {
            let _journal = write.open_table(JOURNAL)?;
            let _index = write.open_table(REQUEST_INDEX)?;
            let _orders = write.open_table(ORDERS)?;
            let _results = write.open_table(RESULTS)?;
            let _executions = write.open_table(EXECUTIONS)?;
            let _balances = write.open_table(BALANCES)?;
            let _inputs = write.open_table(INPUTS)?;
            let _outcomes = write.open_table(OUTCOMES)?;
            let _control = write.open_table(CONTROL)?;
            let _views = write.open_table(EVENT_VIEWS)?;
            let mut secrets = write.open_table(SECRETS)?;
            let absent = secrets.get("view_key")?.is_none();
            if absent {
                let seed = format!(
                    "{}{}",
                    nautilus_core::UUID4::new(),
                    nautilus_core::UUID4::new()
                );
                secrets.insert(
                    "view_key",
                    blake3::hash(seed.as_bytes()).as_bytes().as_slice(),
                )?;
            }
            let mut meta = write.open_table(META)?;
            let schema = meta.get(META_SCHEMA)?.map(|v| v.value());
            let last = meta.get(META_LAST_SEQ)?.map_or(0, |v| v.value());
            if schema.is_some_and(|v| v != 1) || (schema.is_none() && last != 0) {
                return Err(invalid(
                    "unsupported or legacy journal schema; offline migration required",
                ));
            }
            epoch = meta.get(META_EPOCH)?.map_or(1, |v| v.value());
            if epoch == 0 {
                return Err(invalid("journal epoch must be positive"));
            }
            meta.insert(META_SCHEMA, 1)?;
            meta.insert(META_EPOCH, epoch)?;
        }
        write.commit()?;
        let journal = Self {
            db,
            journal_epoch: epoch,
        };
        journal.initialize_recovery()?;
        Ok(journal)
    }

    /// Returns the persistent business epoch.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.journal_epoch
    }

    /// Persists a health transition once; unchanged health does not append an event.
    pub fn commit_control(
        &self,
        control: crate::ledger::ControlRecord,
    ) -> Result<(), JournalError> {
        let current = self
            .db
            .begin_read()?
            .open_table(CONTROL)?
            .get(0)?
            .map(|value| serde_json::from_slice::<crate::ledger::ControlRecord>(value.value()))
            .transpose()?;
        if current.as_ref() != Some(&control) {
            self.append_batch(&[PendingEntry::Control(control)])?;
        }
        Ok(())
    }

    /// Returns the persistent token signing key without exposing it through the API.
    pub(crate) fn view_key(&self) -> Result<[u8; 32], JournalError> {
        let read = self.db.begin_read()?;
        read.open_table(SECRETS)?
            .get("view_key")?
            .ok_or_else(|| invalid("missing view signing key"))?
            .value()
            .try_into()
            .map_err(|_| invalid("invalid view signing key"))
    }

    pub(crate) fn event_view(
        &self,
        seq: u64,
    ) -> Result<Option<(BusinessOrder, RiskBalance)>, JournalError> {
        self.db
            .begin_read()?
            .open_table(EVENT_VIEWS)?
            .get(seq)?
            .map(|bytes| serde_json::from_slice(bytes.value()).map_err(JournalError::from))
            .transpose()
    }

    /// Returns the committed high water.
    pub fn last_seq(&self) -> Result<u64, JournalError> {
        let read = self.db.begin_read()?;
        Ok(read
            .open_table(META)?
            .get(META_LAST_SEQ)?
            .map_or(0, |v| v.value()))
    }

    /// Reads and verifies one committed envelope.
    pub fn entry(&self, seq: u64) -> Result<Option<JournalEntry>, JournalError> {
        let read = self.db.begin_read()?;
        read.open_table(JOURNAL)?
            .get(seq)?
            .map(|v| decode(v.value(), self.epoch(), seq))
            .transpose()
    }

    /// Appends a trusted diagnostic admission with its immutable result.
    pub fn append_admission(
        &self,
        record: AdmissionRecord,
    ) -> Result<AdmissionRecord, JournalError> {
        match self
            .append_batch(&[PendingEntry::Admission(Box::new(record))])?
            .remove(0)
        {
            CommittedEntry::Admission(record) => Ok(*record),
            _ => Err(invalid("unexpected admission commit result")),
        }
    }

    /// Atomically admits an order with its reservation, pending dispatch, and original result.
    pub fn admit_business(
        &self,
        record: AdmissionRecord,
        context: Option<RiskContext>,
        rejection: Option<String>,
    ) -> Result<AdmissionRecord, JournalError> {
        match self
            .append_batch(&[PendingEntry::BusinessAdmission {
                record: Box::new(record),
                context: context.map(Box::new),
                rejection,
            }])?
            .remove(0)
        {
            CommittedEntry::Admission(record) => Ok(*record),
            _ => Err(invalid("unexpected admission commit result")),
        }
    }

    /// Appends an outcome and changes its projection and economic ledger in the same commit.
    pub fn append_outcome(
        &self,
        origin_seq: u64,
        outcome: JournalOutcome,
    ) -> Result<u64, JournalError> {
        match self
            .append_batch(&[PendingEntry::Outcome {
                origin_seq,
                outcome,
            }])?
            .remove(0)
        {
            CommittedEntry::Outcome { seq, .. } => Ok(seq),
            _ => Err(invalid("unexpected outcome commit result")),
        }
    }

    /// Looks up the original immutable request result by its collision-free key.
    pub fn find_request(
        &self,
        submitter_id: &str,
        method: &str,
        request_id: &str,
    ) -> Result<Option<RequestRecord>, JournalError> {
        let read = self.db.begin_read()?;
        let index = read.open_table(REQUEST_INDEX)?;
        let key = Self::request_key_parts(self.epoch(), submitter_id, method, request_id);
        let Some(seq) = index.get(key.as_bytes())?.map(|v| v.value()) else {
            return Ok(None);
        };
        let journal = read.open_table(JOURNAL)?;
        let value = journal
            .get(seq)?
            .ok_or_else(|| invalid("missing indexed admission"))?;
        let JournalEntry::Admission(admission) = decode(value.value(), self.epoch(), seq)? else {
            return Err(invalid("invalid request index"));
        };
        let results = read.open_table(RESULTS)?;
        let result = serde_json::from_slice(
            results
                .get(seq)?
                .ok_or_else(|| invalid("missing request result"))?
                .value(),
        )?;
        let outcomes = read.open_table(OUTCOMES)?;
        let outcome = outcomes
            .get(seq)?
            .map(|v| serde_json::from_slice(v.value()))
            .transpose()?;
        Ok(Some(RequestRecord {
            admission: *admission,
            result,
            outcome,
        }))
    }

    /// Reads a committed order directly from its durable index.
    pub fn order(&self, seq: u64) -> Result<Option<BusinessOrder>, JournalError> {
        let read = self.db.begin_read()?;
        read.open_table(ORDERS)?
            .get(seq)?
            .map(|v| serde_json::from_slice(v.value()).map_err(Into::into))
            .transpose()
    }

    /// Looks up active or terminal orders from one authorized committed revision.
    pub fn lookup_orders(
        &self,
        ids: &[String],
        submitter: &str,
        strategies: &[String],
    ) -> Result<(u64, Vec<BusinessOrder>), JournalError> {
        let read = self.db.begin_read()?;
        let cursor = read
            .open_table(META)?
            .get(META_LAST_SEQ)?
            .map_or(0, |v| v.value());
        let table = read.open_table(ORDERS)?;
        let mut orders = Vec::new();
        for id in ids {
            let seq = nautilus_model::identifiers::ClientOrderId::new_checked(id.as_str())
                .ok()
                .and_then(|value| crate::exec_wiring::seq_from_client_order_id(&value))
                .ok_or_else(|| invalid("order unavailable or unauthorized"))?;
            let order: BusinessOrder = serde_json::from_slice(
                table
                    .get(seq)?
                    .ok_or_else(|| invalid("order unavailable or unauthorized"))?
                    .value(),
            )?;
            if order.client_order_id != *id
                || order.admission.submitter_id != submitter
                || !strategies.contains(&order.admission.strategy_id)
            {
                return Err(invalid("order unavailable or unauthorized"));
            }
            orders.push(order);
        }
        Ok((cursor, orders))
    }

    /// Captures a fixed committed order dataset and its high water in one read transaction.
    pub fn snapshot_orders(
        &self,
        maximum: usize,
    ) -> Result<(u64, Vec<BusinessOrder>), JournalError> {
        let read = self.db.begin_read()?;
        let cursor = read
            .open_table(META)?
            .get(META_LAST_SEQ)?
            .map_or(0, |v| v.value());
        let mut orders = Vec::new();
        for row in read.open_table(ORDERS)?.iter()? {
            if orders.len() >= maximum {
                return Err(invalid("snapshot dataset exceeds configured bound"));
            }
            orders.push(serde_json::from_slice(row?.1.value())?);
        }
        Ok((cursor, orders))
    }

    /// Captures authorized active orders, inputs, and risk balances at a single commit.
    pub fn snapshot(
        &self,
        submitter: &str,
        strategies: &[String],
        maximum: usize,
    ) -> Result<CommittedSnapshot, JournalError> {
        let read = self.db.begin_read()?;
        let cursor = read
            .open_table(META)?
            .get(META_LAST_SEQ)?
            .map_or(0, |v| v.value());
        let mut snapshot = CommittedSnapshot {
            control: read
                .open_table(CONTROL)?
                .get(0)?
                .map(|value| serde_json::from_slice(value.value()))
                .transpose()?,
            cursor,
            orders: Vec::new(),
            inputs: Vec::new(),
            positions: Vec::new(),
            accounts: Vec::new(),
        };
        for row in read.open_table(ORDERS)?.iter()? {
            let order: BusinessOrder = serde_json::from_slice(row?.1.value())?;
            if !order.is_terminal()
                && order.admission.submitter_id == submitter
                && strategies.contains(&order.admission.strategy_id)
            {
                if snapshot.orders.len() >= maximum {
                    return Err(invalid("snapshot order limit exceeded"));
                }
                snapshot.orders.push(order);
            }
        }
        for row in read.open_table(INPUTS)?.iter()? {
            let mut event: crate::inputs::InputEvent = serde_json::from_slice(row?.1.value())?;
            if let crate::inputs::InputEvent::Account { ownership, .. } = &mut event {
                ownership.retain(|holding| strategies.contains(&holding.strategy_id));
                // Account-wide financial facts require a separate future permission
                continue;
            }
            snapshot.inputs.push(event);
        }
        for row in read.open_table(BALANCES)?.iter()? {
            let (key, bytes) = row?;
            let (account, strategy): (String, String) = serde_json::from_str(key.value())?;
            let balance: RiskBalance = serde_json::from_slice(bytes.value())?;
            if strategies.contains(&strategy) {
                snapshot
                    .positions
                    .push(orderhub_proto::orderhub::v1::PositionView {
                        strategy_id: strategy,
                        instrument_id: "AAPL.GOX".into(),
                        quantity: balance.quantity,
                        cost: balance.cost,
                        reserved_quantity: balance.reserved_quantity,
                    });
                snapshot
                    .accounts
                    .push(orderhub_proto::orderhub::v1::AccountView {
                        account_id: account,
                        strategy_id: snapshot
                            .positions
                            .last()
                            .expect("just inserted position")
                            .strategy_id
                            .clone(),
                        reserved_cash: balance.reserved_cash,
                        spent_cash: balance.spent_cash,
                    });
            }
        }
        Ok(snapshot)
    }

    /// Reads a bounded durable event batch without a scan/live handoff.
    pub fn scan_page(
        &self,
        after: u64,
        maximum: usize,
    ) -> Result<Vec<(u64, JournalEntry)>, JournalError> {
        let read = self.db.begin_read()?;
        let journal = read.open_table(JOURNAL)?;
        let mut entries = Vec::new();
        if after == u64::MAX {
            return Ok(entries);
        }
        for row in journal.range(after + 1..)?.take(maximum) {
            let (seq, bytes) = row?;
            entries.push((
                seq.value(),
                decode(bytes.value(), self.epoch(), seq.value())?,
            ));
        }
        Ok(entries)
    }

    /// Scans the complete journal for explicit offline diagnostics.
    pub fn scan_all(&self) -> Result<Vec<(u64, JournalEntry)>, JournalError> {
        self.scan_after(0)
    }

    /// Scans all later entries for explicit offline diagnostics.
    pub fn scan_after(&self, after: u64) -> Result<Vec<(u64, JournalEntry)>, JournalError> {
        self.scan_page(after, usize::MAX)
    }

    /// Commits request indexes, orders, reservations, execution indexes, and events atomically.
    pub fn append_batch(
        &self,
        items: &[PendingEntry],
    ) -> Result<Vec<CommittedEntry>, JournalError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let mut write = self.db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        let mut committed = Vec::with_capacity(items.len());
        {
            let mut journal = write.open_table(JOURNAL)?;
            let mut index = write.open_table(REQUEST_INDEX)?;
            let mut meta = write.open_table(META)?;
            let mut orders = write.open_table(ORDERS)?;
            let mut results = write.open_table(RESULTS)?;
            let mut executions = write.open_table(EXECUTIONS)?;
            let mut balances = write.open_table(BALANCES)?;
            let mut inputs = write.open_table(INPUTS)?;
            let mut outcomes = write.open_table(OUTCOMES)?;
            let mut control = write.open_table(CONTROL)?;
            let mut event_views = write.open_table(EVENT_VIEWS)?;
            let mut recovery = write.open_table(RECOVERY)?;
            let mut recovery_commits = write.open_table(RECOVERY_COMMITS)?;
            let mut last_seq = meta.get(META_LAST_SEQ)?.map_or(0, |v| v.value());
            let mut stored_bytes = meta.get(META_BYTES)?.map_or(0, |v| v.value());
            for item in items {
                let mut recovery_mutations = Vec::new();
                macro_rules! track {
                    ($table:ident, $key:expr, $value:expr $(,)?) => {{
                        let recovery_value = ($value).to_vec();
                        $table.insert($key, recovery_value.as_slice())?;
                        recovery_mutations.push(Mutation::bytes(
                            stringify!($table),
                            RowKey::from($key),
                            recovery_value,
                        ));
                    }};
                }
                let next = last_seq
                    .checked_add(1)
                    .ok_or_else(|| invalid("sequence overflow"))?;
                let entry = match item {
                    PendingEntry::Control(value) => {
                        track!(control, 0, serde_json::to_vec(value)?.as_slice());
                        committed.push(CommittedEntry::Control { seq: next });
                        JournalEntry::Control(value.clone())
                    }
                    PendingEntry::Input {
                        event,
                        received_at_ns,
                    } => {
                        let key = match event.as_ref() {
                            crate::inputs::InputEvent::Instrument { .. } => "instrument",
                            crate::inputs::InputEvent::Quote { .. } => "quote",
                            crate::inputs::InputEvent::Status { .. } => "status",
                            crate::inputs::InputEvent::Account { .. } => "account",
                        };
                        track!(inputs, key, serde_json::to_vec(event)?.as_slice());
                        if let crate::inputs::InputEvent::Account {
                            account_id,
                            ownership,
                            position,
                            ..
                        } = event.as_ref()
                        {
                            let account_key = balance_key(account_id, "");
                            if balances.get(account_key.as_str())?.is_none() {
                                let value = RiskBalance::initial(
                                    decimal(position)?,
                                    rust_decimal::Decimal::ZERO,
                                )?;
                                track!(
                                    balances,
                                    account_key.as_str(),
                                    serde_json::to_vec(&value)?.as_slice(),
                                );
                            }
                            for holding in ownership {
                                let key = balance_key(account_id, &holding.strategy_id);
                                if balances.get(key.as_str())?.is_none()
                                    && let Some(cost) = &holding.cost_basis
                                {
                                    let value = RiskBalance::initial(
                                        decimal(&holding.quantity)?,
                                        decimal(cost)?,
                                    )?;
                                    track!(
                                        balances,
                                        key.as_str(),
                                        serde_json::to_vec(&value)?.as_slice(),
                                    );
                                }
                            }
                        }
                        committed.push(CommittedEntry::Input { seq: next });
                        JournalEntry::Input {
                            event: event.clone(),
                            received_at_ns: *received_at_ns,
                        }
                    }
                    PendingEntry::Admission(record)
                    | PendingEntry::BusinessAdmission { record, .. } => {
                        let key = Self::request_key_parts(
                            self.epoch(),
                            &record.submitter_id,
                            &record.request_kind,
                            &record.request_id,
                        );
                        if let Some(old_seq) = index.get(key.as_bytes())?.map(|v| v.value()) {
                            let bytes = journal
                                .get(old_seq)?
                                .ok_or_else(|| invalid("missing indexed admission"))?;
                            let JournalEntry::Admission(old) =
                                decode(bytes.value(), self.epoch(), old_seq)?
                            else {
                                return Err(invalid("invalid index"));
                            };
                            if old.request_fingerprint != record.request_fingerprint {
                                return Err(invalid("IDEMPOTENCY_CONFLICT"));
                            }
                            committed.push(CommittedEntry::Admission(old));
                            continue;
                        }
                        let mut record = (**record).clone();
                        record.seq = next;
                        record.journal_epoch = self.epoch();
                        let (context, mut rejection) = match item {
                            PendingEntry::BusinessAdmission {
                                context, rejection, ..
                            } => (context.as_deref(), rejection.clone()),
                            _ => (None, None),
                        };
                        let mut client_order_id = if record.request_kind == "CancelOrder" {
                            record
                                .instrument_id
                                .strip_prefix("CANCEL:")
                                .unwrap_or_default()
                                .to_string()
                        } else {
                            format!("O-{}-{:012}", self.epoch(), next)
                        };
                        if let Some(context) = context {
                            let mut order = BusinessOrder::new(record.clone(), context)?;
                            let strategy_key =
                                balance_key(&context.account_id, &record.strategy_id);
                            let account_key = balance_key(&context.account_id, "");
                            let mut strategy: RiskBalance =
                                match balances.get(strategy_key.as_str())? {
                                    Some(v) => serde_json::from_slice(v.value())?,
                                    None => RiskBalance::initial(
                                        context.owned_quantity,
                                        context.owned_cost,
                                    )?,
                                };
                            let mut account: RiskBalance =
                                match balances.get(account_key.as_str())? {
                                    Some(v) => serde_json::from_slice(v.value())?,
                                    None => RiskBalance::initial(
                                        context.account_quantity,
                                        rust_decimal::Decimal::ZERO,
                                    )?,
                                };
                            if rejection.is_none() {
                                rejection = risk_rejection(&order, context, &strategy, &account)?;
                            }
                            if rejection.is_none() {
                                strategy.update(None, &order)?;
                                account.update(None, &order)?;
                                track!(
                                    balances,
                                    strategy_key.as_str(),
                                    serde_json::to_vec(&strategy)?.as_slice(),
                                );
                                track!(
                                    balances,
                                    account_key.as_str(),
                                    serde_json::to_vec(&account)?.as_slice(),
                                );
                                order.version = next;
                                track!(orders, next, serde_json::to_vec(&order)?.as_slice());
                            }
                        }
                        let mut result = if let Some(reason) = rejection {
                            client_order_id.clear();
                            RequestResult {
                                result: "REJECTED".into(),
                                reason,
                                client_order_id,
                                version: next,
                            }
                        } else {
                            RequestResult {
                                result: if record.request_kind == "CancelOrder" {
                                    "CANCEL_INTENT_ACCEPTED".into()
                                } else {
                                    "ADMITTED".into()
                                },
                                reason: String::new(),
                                client_order_id,
                                version: next,
                            }
                        };
                        if record.request_kind == "CancelOrder" && result.result != "REJECTED" {
                            let target_seq = result
                                .client_order_id
                                .rsplit('-')
                                .next()
                                .and_then(|value| value.parse::<u64>().ok())
                                .ok_or_else(|| invalid("invalid cancel target"))?;
                            let before: Option<BusinessOrder> = orders
                                .get(target_seq)?
                                .map(|bytes| serde_json::from_slice(bytes.value()))
                                .transpose()?;
                            if let Some(before) = before {
                                if before.admission.submitter_id != record.submitter_id {
                                    return Err(invalid("unauthorized cancellation"));
                                }
                                let mut order = before.clone();
                                if order.is_terminal() {
                                    result.result = "ALREADY_TERMINAL".into();
                                    result.reason.clone_from(&order.status);
                                } else if order.cancel_pending {
                                    result.reason = "CANCEL_ALREADY_PENDING".into();
                                } else {
                                    order.cancel_pending = true;
                                    if !order.dispatch_started {
                                        order.status = "CANCELED".into();
                                        order.cancel_pending = false;
                                        order.fee_buffer = "0".into();
                                        order.recalculate()?;
                                        result.reason = "LOCAL_CANCELED".into();
                                    }
                                    order.version = next;
                                    for strategy in [order.admission.strategy_id.as_str(), ""] {
                                        let key = balance_key(&order.account_id, strategy);
                                        let mut balance: RiskBalance = serde_json::from_slice(
                                            balances
                                                .get(key.as_str())?
                                                .ok_or_else(|| invalid("missing risk balance"))?
                                                .value(),
                                        )?;
                                        balance.update(Some(&before), &order)?;
                                        track!(
                                            balances,
                                            key.as_str(),
                                            serde_json::to_vec(&balance)?.as_slice(),
                                        );
                                    }
                                    track!(
                                        orders,
                                        target_seq,
                                        serde_json::to_vec(&order)?.as_slice(),
                                    );
                                }
                            }
                        }
                        let initial = if result.result == "REJECTED" {
                            JournalOutcome::Denied {
                                reason: result.reason.clone(),
                            }
                        } else {
                            JournalOutcome::Admitted {
                                client_order_id: result.client_order_id.clone(),
                            }
                        };
                        track!(outcomes, next, serde_json::to_vec(&initial)?.as_slice());
                        index.insert(key.as_bytes(), next)?;
                        recovery_mutations.push(Mutation::number(
                            "index",
                            RowKey::from(key.as_bytes()),
                            next,
                        ));
                        track!(results, next, serde_json::to_vec(&result)?.as_slice());
                        committed.push(CommittedEntry::Admission(Box::new(record.clone())));
                        JournalEntry::Admission(Box::new(record))
                    }
                    PendingEntry::Outcome {
                        origin_seq,
                        outcome,
                    } => {
                        let before: Option<BusinessOrder> = orders
                            .get(*origin_seq)?
                            .map(|v| serde_json::from_slice(v.value()))
                            .transpose()?;
                        if let Some(mut order) = before.clone() {
                            match outcome {
                                JournalOutcome::DispatchStarted { client_order_id } => {
                                    if client_order_id != &order.client_order_id
                                        || order.is_terminal()
                                        || order.dispatch_started
                                    {
                                        return Err(invalid("invalid dispatch transition"));
                                    }
                                    order.dispatch_started = true;
                                    order.status = "DISPATCH_STARTED".into();
                                }
                                JournalOutcome::Denied { .. } => {
                                    if order.status != "FILLED" {
                                        order.status = "REJECTED".into();
                                    }
                                    if decimal(&order.filled_quantity)?
                                        == rust_decimal::Decimal::ZERO
                                    {
                                        order.fee_buffer = "0".into();
                                    }
                                    order.recalculate()?;
                                }
                                JournalOutcome::Canceled { .. } => {
                                    if order.status != "FILLED" {
                                        order.status = "CANCELED".into();
                                    }
                                    order.recalculate()?;
                                }
                                JournalOutcome::Execution(event) => {
                                    let identity = execution_key(event);
                                    if let Some(old) = executions.get(identity.as_str())? {
                                        let old: ExecutionRecord =
                                            serde_json::from_slice(old.value())?;
                                        let mut normalized = (**event).clone();
                                        normalized.event_id.clone_from(&old.event_id);
                                        normalized.ts_received_ns = old.ts_received_ns;
                                        normalized.ts_event_ns = old.ts_event_ns;
                                        normalized.native_event.clone_from(&old.native_event);
                                        if old != normalized {
                                            return Err(invalid("conflicting execution identity"));
                                        }
                                        committed.push(CommittedEntry::Outcome {
                                            seq: last_seq,
                                            origin_seq: *origin_seq,
                                        });
                                        continue;
                                    }
                                    order.apply(event, next)?;
                                    track!(
                                        executions,
                                        identity.as_str(),
                                        serde_json::to_vec(event)?.as_slice(),
                                    );
                                }
                                JournalOutcome::Admitted { .. } => {}
                                JournalOutcome::Filled { .. } => {
                                    return Err(invalid(
                                        "quantity-only fill is insufficient for the business ledger",
                                    ));
                                }
                            }
                            order.version = next;
                            for strategy in [order.admission.strategy_id.as_str(), ""] {
                                let key = balance_key(&order.account_id, strategy);
                                let mut balance: RiskBalance = serde_json::from_slice(
                                    balances
                                        .get(key.as_str())?
                                        .ok_or_else(|| invalid("missing risk balance"))?
                                        .value(),
                                )?;
                                balance.update(before.as_ref(), &order)?;
                                track!(
                                    balances,
                                    key.as_str(),
                                    serde_json::to_vec(&balance)?.as_slice(),
                                );
                            }
                            track!(orders, *origin_seq, serde_json::to_vec(&order)?.as_slice());
                        } else if matches!(outcome, JournalOutcome::Execution(_)) {
                            return Err(invalid("execution has no committed order"));
                        }
                        track!(
                            outcomes,
                            *origin_seq,
                            serde_json::to_vec(outcome)?.as_slice()
                        );
                        committed.push(CommittedEntry::Outcome {
                            seq: next,
                            origin_seq: *origin_seq,
                        });
                        JournalEntry::Outcome {
                            origin_seq: *origin_seq,
                            outcome: outcome.clone(),
                        }
                    }
                };
                let mut view_bytes = 0_u64;
                let projected_seq = match &entry {
                    JournalEntry::Admission(record) if record.request_kind == "CancelOrder" => {
                        record
                            .instrument_id
                            .rsplit('-')
                            .next()
                            .and_then(|value| value.parse().ok())
                    }
                    JournalEntry::Admission(_) => Some(next),
                    JournalEntry::Outcome { origin_seq, .. } => Some(*origin_seq),
                    _ => None,
                };
                if let Some(origin) = projected_seq
                    && let Some(bytes) = orders.get(origin)?
                {
                    let order: BusinessOrder = serde_json::from_slice(bytes.value())?;
                    let key = balance_key(&order.account_id, &order.admission.strategy_id);
                    let balance: RiskBalance = serde_json::from_slice(
                        balances
                            .get(key.as_str())?
                            .ok_or_else(|| invalid("missing projected balance"))?
                            .value(),
                    )?;
                    let bytes = serde_json::to_vec(&(order, balance))?;
                    view_bytes = bytes.len() as u64;
                    track!(event_views, next, bytes.as_slice());
                }
                let bytes = encode(&entry, self.epoch(), next)?;
                let recovery_size = serde_json::to_vec(&recovery_mutations)?.len() as u64
                    + (bytes.len() as u64).saturating_mul(4)
                    + 512;
                stored_bytes = stored_bytes
                    .checked_add(bytes.len() as u64)
                    .and_then(|value| value.checked_add(view_bytes))
                    .and_then(|value| value.checked_add(recovery_size))
                    .ok_or_else(|| invalid("journal byte count overflow"))?;
                let reserve_required = matches!(&entry, JournalEntry::Input { .. })
                    || matches!(&entry, JournalEntry::Admission(record) if record.request_kind == "SubmitOrder");
                if next > MAXIMUM_RECORDS
                    || stored_bytes > MAXIMUM_BYTES
                    || (reserve_required && stored_bytes > ADMISSION_BYTES)
                {
                    return Err(JournalError::Storage(
                        "journal retention limit reached; checkpoint/archive activation required"
                            .into(),
                    ));
                }
                journal.insert(next, bytes.as_slice())?;
                recovery_mutations.push(Mutation::bytes("journal", RowKey::from(next), bytes));
                recovery_mutations.push(Mutation::number(
                    "meta",
                    RowKey::from(META_LAST_SEQ),
                    next,
                ));
                recovery_mutations.push(Mutation::number(
                    "meta",
                    RowKey::from(META_BYTES),
                    stored_bytes,
                ));
                recovery_storage::commit_delta(
                    &mut recovery,
                    &mut recovery_commits,
                    next,
                    recovery_mutations,
                )?;
                last_seq = next;
            }
            meta.insert(META_LAST_SEQ, last_seq)?;
            meta.insert(META_BYTES, stored_bytes)?;
        }
        write.commit()?;
        Ok(committed)
    }

    fn request_key_parts(epoch: u64, submitter_id: &str, method: &str, request_id: &str) -> String {
        serde_json::to_string(&(epoch, submitter_id, method, request_id))
            .expect("string tuple serialization")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    journal_epoch: u64,
    journal_seq: u64,
    kind: String,
    payload: String,
    checksum: String,
}

fn encode(entry: &JournalEntry, epoch: u64, seq: u64) -> Result<Vec<u8>, JournalError> {
    let payload = serde_json::to_string(entry)?;
    let checksum = blake3::hash(payload.as_bytes()).to_hex().to_string();
    Ok(serde_json::to_vec(&Envelope {
        schema_version: 1,
        journal_epoch: epoch,
        journal_seq: seq,
        kind: entry_kind(entry).into(),
        payload,
        checksum,
    })?)
}

fn decode(bytes: &[u8], epoch: u64, seq: u64) -> Result<JournalEntry, JournalError> {
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    if envelope.schema_version != 1
        || envelope.journal_epoch != epoch
        || envelope.journal_seq != seq
        || blake3::hash(envelope.payload.as_bytes()).to_hex().as_str() != envelope.checksum
    {
        return Err(invalid(
            "invalid schema, envelope identity, or persisted payload checksum",
        ));
    }
    let entry: JournalEntry = serde_json::from_str(&envelope.payload)?;
    if envelope.kind != entry_kind(&entry) {
        return Err(invalid("envelope kind mismatch"));
    }
    Ok(entry)
}

fn entry_kind(entry: &JournalEntry) -> &'static str {
    match entry {
        JournalEntry::Admission(_) => "admission",
        JournalEntry::Outcome { .. } => "outcome",
        JournalEntry::Input { .. } => "input",
        JournalEntry::Control(_) => "control",
    }
}

fn balance_key(account: &str, strategy: &str) -> String {
    serde_json::to_string(&(account, strategy)).expect("string tuple serialization")
}

fn execution_key(event: &ExecutionRecord) -> String {
    serde_json::to_string(&(
        &event.account_id,
        &event.instrument_id,
        &event.kind,
        event.trade_id.as_ref().unwrap_or(&event.event_id),
    ))
    .expect("string tuple serialization")
}

fn risk_rejection(
    order: &BusinessOrder,
    context: &RiskContext,
    strategy: &RiskBalance,
    account: &RiskBalance,
) -> Result<Option<String>, JournalError> {
    use rust_decimal::Decimal;
    let qty = decimal(&order.admission.quantity)?;
    let px = decimal(&order.admission.price)?;
    let notional = mul(qty, px)?;
    let cash = decimal(&order.reserved_cash)?;
    let reason = if account.active_orders >= 4_096 {
        Some("ACTIVE_ORDER_LIMIT")
    } else if qty > context.maximum_quantity || notional > context.maximum_notional {
        Some("SINGLE_ORDER_LIMIT")
    } else if order.admission.order_side == "BUY"
        && (add(
            add(
                mul(decimal(&strategy.quantity)?, context.reference_price)?,
                decimal(&strategy.reserved_cash)?,
            )?,
            cash,
        )? > context.strategy_exposure_limit
            || add(
                add(
                    mul(
                        decimal(&account.quantity)?.max(context.account_quantity),
                        context.reference_price,
                    )?,
                    decimal(&account.reserved_cash)?,
                )?,
                cash,
            )? > context.account_exposure_limit)
    {
        Some("GROSS_EXPOSURE_LIMIT")
    } else if add(
        add(
            decimal(&account.spent_cash)?,
            decimal(&account.reserved_cash)?,
        )?,
        cash,
    )? > context.account_budget
    {
        Some("ACCOUNT_BUDGET")
    } else if add(
        add(
            add(decimal(&strategy.cost)?, decimal(&strategy.paid_fees)?)?,
            decimal(&strategy.reserved_cash)?,
        )?,
        cash,
    )? > context.strategy_budget
    {
        Some("STRATEGY_BUDGET")
    } else if order.admission.order_side == "SELL"
        && (qty
            > sub(
                decimal(&strategy.quantity)?.min(context.owned_quantity),
                decimal(&strategy.reserved_quantity)?,
            )?
            .max(Decimal::ZERO)
            || qty
                > sub(
                    decimal(&account.quantity)?.min(context.account_quantity),
                    decimal(&account.reserved_quantity)?,
                )?
                .max(Decimal::ZERO))
    {
        Some("SELLABLE_QUANTITY")
    } else {
        None
    };
    Ok(reason.map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_epoch_is_rejected_before_opening_business_storage() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        let write = db.begin_write().unwrap();
        write
            .open_table(META)
            .unwrap()
            .insert(META_EPOCH, 0)
            .unwrap();
        write.commit().unwrap();
        assert!(matches!(
            BusinessJournal::from_database(db),
            Err(JournalError::Encoding(_))
        ));
    }

    #[test]
    fn request_key_parts_are_unambiguous() {
        let a = BusinessJournal::request_key_parts(1, "s-1", "SubmitOrder", "r-1");
        let b = BusinessJournal::request_key_parts(1, "s-1|SubmitOrder", "", "r-1");
        assert_ne!(a, b);
        assert_eq!(a, r#"[1,"s-1","SubmitOrder","r-1"]"#);
    }

    #[test]
    fn test_payload_corruption_is_rejected_and_health_is_committed_once() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        let journal = BusinessJournal::from_database(db).unwrap();
        let health = crate::ledger::ControlRecord {
            readiness: "READY".into(),
            writable: true,
            connected: Some(true),
            reason: String::new(),
        };
        journal.commit_control(health.clone()).unwrap();
        journal.commit_control(health).unwrap();
        assert_eq!(journal.last_seq().unwrap(), 1);
        let write = journal.db.begin_write().unwrap();
        {
            let mut table = write.open_table(JOURNAL).unwrap();
            let bytes = table.get(1).unwrap().unwrap().value().to_vec();
            let mut envelope: Envelope = serde_json::from_slice(&bytes).unwrap();
            envelope.payload = envelope.payload.replace("READY", "HALTED");
            table
                .insert(1, serde_json::to_vec(&envelope).unwrap().as_slice())
                .unwrap();
        }
        write.commit().unwrap();
        assert!(matches!(journal.entry(1), Err(JournalError::Encoding(_))));
    }

    #[test]
    fn test_retention_limit_does_not_publish_failed_control_transition() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        let journal = BusinessJournal::from_database(db).unwrap();
        let write = journal.db.begin_write().unwrap();
        write
            .open_table(META)
            .unwrap()
            .insert(META_BYTES, MAXIMUM_BYTES)
            .unwrap();
        write.commit().unwrap();
        assert!(
            journal
                .commit_control(crate::ledger::ControlRecord {
                    readiness: "READY".into(),
                    writable: true,
                    connected: Some(true),
                    reason: String::new()
                })
                .is_err()
        );
        assert_eq!(journal.last_seq().unwrap(), 0);
        assert!(
            journal
                .snapshot("owner", &[], 10)
                .unwrap()
                .control
                .is_none()
        );
    }
}
