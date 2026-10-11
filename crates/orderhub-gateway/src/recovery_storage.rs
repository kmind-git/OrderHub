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

//! Verified logical checkpoints and atomic recovery deltas for the business database.

use nautilus_core::UUID4;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

pub(super) const RECOVERY: TableDefinition<&str, &[u8]> =
    TableDefinition::new("orderhub_recovery_v2");
pub(super) const RECOVERY_COMMITS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("orderhub_recovery_commits_v2");
const MAX_BLOB_BYTES: u64 = 512 * 1024 * 1024;
const RESTORE_FENCE: u64 = 6;

/// Identity attached to an immutable recovery artifact, without credential secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryIdentity {
    pub instance_id: String,
    pub binary_hash: String,
    pub configuration_hash: String,
}

/// Manifest published only after its immutable logical database image is synchronized.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryManifest {
    pub format_version: u32,
    pub journal_epoch: u64,
    pub cursor: u64,
    pub blob: String,
    pub blob_bytes: u64,
    pub blake3: String,
    pub identity: RecoveryIdentity,
}

/// Verified startup state; uncertain dispatches are never automatically resent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryReport {
    pub epoch: u64,
    pub cursor: u64,
    pub base_cursor: u64,
    pub replayed_commits: usize,
    pub business_orders: usize,
    pub uncertain_orders: Vec<String>,
    pub unsent_orders: Vec<String>,
    pub backup_restore_fenced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RowKey {
    Number(u64),
    Text(String),
    Bytes(Vec<u8>),
}

impl From<u64> for RowKey {
    fn from(value: u64) -> Self {
        Self::Number(value)
    }
}
impl From<&str> for RowKey {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<&[u8]> for RowKey {
    fn from(value: &[u8]) -> Self {
        Self::Bytes(value.to_vec())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum RowValue {
    Bytes(Vec<u8>),
    Number(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    table: String,
    key: RowKey,
    value: RowValue,
}

impl Mutation {
    pub(super) fn bytes(table: &str, key: RowKey, value: Vec<u8>) -> Self {
        Self {
            table: table.into(),
            key,
            value: RowValue::Bytes(value),
        }
    }
    pub(super) fn number(table: &str, key: RowKey, value: u64) -> Self {
        Self {
            table: table.into(),
            key,
            value: RowValue::Number(value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Image {
    format_version: u32,
    rows: Vec<Mutation>,
}

impl Image {
    fn number(&self, key: u64) -> u64 {
        self.rows
            .iter()
            .find_map(|row| {
                if row.table == "meta"
                    && row.key == RowKey::Number(key)
                    && let RowValue::Number(value) = &row.value
                {
                    return Some(*value);
                }
                None
            })
            .unwrap_or(0)
    }
    fn merge(&mut self, changes: Vec<Mutation>) {
        let mut rows: BTreeMap<_, _> = self
            .rows
            .drain(..)
            .map(|row| ((row.table.clone(), row.key.clone()), row))
            .collect();
        for row in changes {
            rows.insert((row.table.clone(), row.key.clone()), row);
        }
        self.rows = rows.into_values().collect();
    }
    fn validate(&self) -> Result<(), JournalError> {
        if self.format_version != 2 || self.number(META_SCHEMA) != 1 || self.number(META_EPOCH) == 0
        {
            return Err(invalid("unsupported recovery image schema or epoch"));
        }
        let mut expected = 1;
        for row in &self.rows {
            if row.table == "journal" {
                let (RowKey::Number(seq), RowValue::Bytes(bytes)) = (&row.key, &row.value) else {
                    return Err(invalid("invalid recovered journal row"));
                };
                if *seq != expected {
                    return Err(invalid("journal history is not contiguous"));
                }
                decode(bytes, self.number(META_EPOCH), *seq)?;
                expected += 1;
            }
            if row.table == "orders" {
                let RowValue::Bytes(bytes) = &row.value else {
                    return Err(invalid("invalid recovered order"));
                };
                let order: BusinessOrder = serde_json::from_slice(bytes)?;
                if row.key != RowKey::Number(order.admission.seq) {
                    return Err(invalid("recovered order identity mismatch"));
                }
            }
        }
        if expected - 1 != self.number(META_LAST_SEQ) {
            return Err(invalid("journal tail is incomplete"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Delta {
    seq: u64,
    previous: String,
    mutations: Vec<Mutation>,
    hash: String,
}

fn delta_hash(seq: u64, previous: &str, mutations: &[Mutation]) -> Result<String, JournalError> {
    Ok(
        blake3::hash(&serde_json::to_vec(&(seq, previous, mutations))?)
            .to_hex()
            .to_string(),
    )
}

pub(super) fn commit_delta(
    recovery: &mut redb::Table<'_, &str, &[u8]>,
    commits: &mut redb::Table<'_, u64, &[u8]>,
    seq: u64,
    mutations: Vec<Mutation>,
) -> Result<(), JournalError> {
    let previous = String::from_utf8(
        recovery
            .get("head")?
            .ok_or_else(|| invalid("missing recovery head"))?
            .value()
            .to_vec(),
    )
    .map_err(|e| invalid(&e.to_string()))?;
    let hash = delta_hash(seq, &previous, &mutations)?;
    let delta = Delta {
        seq,
        previous,
        mutations,
        hash: hash.clone(),
    };
    commits.insert(seq, serde_json::to_vec(&delta)?.as_slice())?;
    recovery.insert("head", hash.as_bytes())?;
    Ok(())
}

impl BusinessJournal {
    pub(super) fn initialize_recovery(&self) -> Result<(), JournalError> {
        let mut write = self.db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        let absent = write.open_table(RECOVERY)?.get("base")?.is_none();
        let version = write.open_table(META)?.get(5)?.map(|value| value.value());
        if version.is_some_and(|value| value != 2) || (version == Some(2) && absent) {
            return Err(invalid(
                "unsupported recovery version or missing sealed recovery base",
            ));
        }
        if write.open_table(RECOVERY)?.get("instance_id")?.is_none() {
            write
                .open_table(RECOVERY)?
                .insert("instance_id", UUID4::new().to_string().as_bytes())?;
        }
        {
            let _ = write.open_table(RECOVERY_COMMITS)?;
        }
        write.commit()?;
        if absent {
            let mut image = self.capture_image()?;
            image.merge(vec![Mutation::number("meta", RowKey::Number(5), 2)]);
            image.validate()?;
            self.install_base(&image)?;
        }
        Ok(())
    }

    fn install_base(&self, image: &Image) -> Result<(), JournalError> {
        let bytes = serde_json::to_vec(image)?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let mut write = self.db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        {
            let mut recovery = write.open_table(RECOVERY)?;
            recovery.insert("base", bytes.as_slice())?;
            recovery.insert("base_hash", hash.as_bytes())?;
            recovery.insert("head", hash.as_bytes())?;
        }
        write.open_table(RECOVERY_COMMITS)?.retain(|_, _| false)?;
        write.open_table(META)?.insert(5, 2)?;
        write.commit()?;
        Ok(())
    }

    fn capture_image(&self) -> Result<Image, JournalError> {
        let read = self.db.begin_read()?;
        let mut image = Image {
            format_version: 2,
            rows: Vec::new(),
        };
        macro_rules! capture_bytes {
            ($definition:expr, $name:expr) => {
                for entry in read.open_table($definition)?.iter()? {
                    let (key, value) = entry?;
                    image.rows.push(Mutation::bytes(
                        $name,
                        RowKey::from(key.value()),
                        value.value().to_vec(),
                    ));
                }
            };
        }
        capture_bytes!(JOURNAL, "journal");
        capture_bytes!(ORDERS, "orders");
        capture_bytes!(RESULTS, "results");
        capture_bytes!(OUTCOMES, "outcomes");
        capture_bytes!(EVENT_VIEWS, "event_views");
        capture_bytes!(CONTROL, "control");
        capture_bytes!(INPUTS, "inputs");
        capture_bytes!(BALANCES, "balances");
        capture_bytes!(EXECUTIONS, "executions");
        capture_bytes!(SECRETS, "secrets");
        for entry in read.open_table(META)?.iter()? {
            let (key, value) = entry?;
            image.rows.push(Mutation::number(
                "meta",
                RowKey::from(key.value()),
                value.value(),
            ));
        }
        for entry in read.open_table(REQUEST_INDEX)?.iter()? {
            let (key, value) = entry?;
            image.rows.push(Mutation::number(
                "index",
                RowKey::from(key.value()),
                value.value(),
            ));
        }
        image.merge(Vec::new());
        Ok(image)
    }

    /// Verifies the sealed recovery base and every atomic delta against current projections.
    ///
    /// # Errors
    /// Returns corruption, incomplete history, or storage errors; it never permits resending.
    pub fn verify_recovery(&self) -> Result<RecoveryReport, JournalError> {
        let read = self.db.begin_read()?;
        let recovery = read.open_table(RECOVERY)?;
        let base = recovery
            .get("base")?
            .ok_or_else(|| invalid("missing recovery base"))?;
        let mut image: Image = serde_json::from_slice(base.value())?;
        let mut head = blake3::hash(base.value()).to_hex().to_string();
        if recovery
            .get("base_hash")?
            .is_none_or(|value| value.value() != head.as_bytes())
        {
            return Err(invalid("recovery base checksum mismatch"));
        }
        let base_cursor = image.number(META_LAST_SEQ);
        let mut cursor = base_cursor;
        let mut replayed = 0;
        for entry in read.open_table(RECOVERY_COMMITS)?.iter()? {
            let (key, bytes) = entry?;
            let delta: Delta = serde_json::from_slice(bytes.value())?;
            if key.value() != cursor + 1
                || delta.seq != key.value()
                || delta.previous != head
                || delta.hash != delta_hash(delta.seq, &delta.previous, &delta.mutations)?
            {
                return Err(invalid("recovery delta chain is incomplete or corrupted"));
            }
            cursor = delta.seq;
            head = delta.hash;
            image.merge(delta.mutations);
            replayed += 1;
        }
        if recovery
            .get("head")?
            .is_none_or(|value| value.value() != head.as_bytes())
        {
            return Err(invalid("recovery head mismatch"));
        }
        image.validate()?;
        // Startup is single-writer: no live dispatch starts before this comparison
        if image != self.capture_image()? {
            return Err(invalid("recovered projections differ from durable tables"));
        }
        let mut report = RecoveryReport {
            epoch: self.epoch(),
            cursor,
            base_cursor,
            replayed_commits: replayed,
            business_orders: 0,
            uncertain_orders: Vec::new(),
            unsent_orders: Vec::new(),
            backup_restore_fenced: image.number(RESTORE_FENCE) != 0,
        };
        for row in &image.rows {
            if row.table == "orders" {
                let RowValue::Bytes(bytes) = &row.value else {
                    return Err(invalid("invalid order recovery bytes"));
                };
                let order: BusinessOrder = serde_json::from_slice(bytes)?;
                report.business_orders += 1;
                if !order.is_terminal() {
                    if order.dispatch_started {
                        report.uncertain_orders.push(order.client_order_id);
                    } else {
                        report.unsent_orders.push(order.client_order_id);
                    }
                }
            }
        }
        Ok(report)
    }

    /// Returns the persisted identity of this local journal instance.
    pub fn instance_id(&self) -> Result<String, JournalError> {
        let read = self.db.begin_read()?;
        let bytes = read
            .open_table(RECOVERY)?
            .get("instance_id")?
            .ok_or_else(|| invalid("missing recovery instance identity"))?
            .value()
            .to_vec();
        String::from_utf8(bytes).map_err(|e| invalid(&e.to_string()))
    }

    /// Publishes a consistent logical checkpoint without copying the live database file.
    ///
    /// # Errors
    /// Returns validation, file, or synchronization errors. Existing blobs are immutable.
    pub fn checkpoint(
        &self,
        directory: &Path,
        identity: RecoveryIdentity,
    ) -> Result<PathBuf, JournalError> {
        self.verify_recovery()?;
        let image = self.capture_image()?;
        image.validate()?;
        publish_image(directory, &image, identity)
    }

    /// Verifies a checkpoint prefix and replays its complete committed tail.
    ///
    /// # Errors
    /// Returns incompatible identity, missing tail, or corruption errors. The caller may
    /// explicitly fall back to verify_recovery when the full sealed base remains intact.
    pub fn verify_checkpoint(&self, manifest_path: &Path) -> Result<RecoveryReport, JournalError> {
        let (manifest, mut image) = load_image(manifest_path)?;
        if manifest.journal_epoch != self.epoch() {
            return Err(invalid("checkpoint epoch mismatch"));
        }
        let read = self.db.begin_read()?;
        let mut cursor = manifest.cursor;
        for entry in read.open_table(RECOVERY_COMMITS)?.range((cursor + 1)..)? {
            let (key, bytes) = entry?;
            if key.value() != cursor + 1 {
                return Err(invalid("checkpoint tail gap"));
            }
            let delta: Delta = serde_json::from_slice(bytes.value())?;
            if delta.hash != delta_hash(delta.seq, &delta.previous, &delta.mutations)? {
                return Err(invalid("checkpoint tail checksum mismatch"));
            }
            image.merge(delta.mutations);
            cursor = key.value();
        }
        image.validate()?;
        if image != self.capture_image()? {
            return Err(invalid(
                "checkpoint and tail do not match committed projections",
            ));
        }
        self.verify_recovery()
    }

    /// Restores an immutable backup into a new database and fences its new journal epoch.
    ///
    /// # Errors
    /// Returns invalid backup or existing-target errors. It never overwrites a live database.
    pub fn restore_backup(manifest_path: &Path, target: &Path) -> Result<Self, JournalError> {
        if target.exists() {
            return Err(invalid("backup restore requires an absent target"));
        }
        let (manifest, mut image) = load_image(manifest_path)?;
        let epoch = fresh_epoch(manifest.journal_epoch);
        for row in &mut image.rows {
            if row.table == "journal" {
                let RowValue::Bytes(bytes) = &mut row.value else {
                    return Err(invalid("invalid journal bytes"));
                };
                let mut envelope: Envelope = serde_json::from_slice(bytes)?;
                envelope.journal_epoch = epoch;
                *bytes = serde_json::to_vec(&envelope)?;
            } else if row.table == "index" {
                let RowKey::Bytes(key) = &mut row.key else {
                    return Err(invalid("invalid request index"));
                };
                let (_, submitter, method, request): (u64, String, String, String) =
                    serde_json::from_slice(key)?;
                *key = serde_json::to_vec(&(epoch, submitter, method, request))?;
            } else if row.table == "secrets" && row.key == RowKey::Text("view_key".into()) {
                row.value = RowValue::Bytes(
                    blake3::hash(UUID4::new().to_string().as_bytes())
                        .as_bytes()
                        .to_vec(),
                );
            }
        }
        image.merge(vec![
            Mutation::number("meta", RowKey::Number(META_EPOCH), epoch),
            Mutation::number("meta", RowKey::Number(RESTORE_FENCE), 1),
        ]);
        image.validate()?;
        let parent = target
            .parent()
            .ok_or_else(|| invalid("restore target has no parent"))?;
        fs::create_dir_all(parent).map_err(|e| file_error(&e))?;
        let staging = parent.join(format!("{}.pending.redb", UUID4::new()));
        let db = Database::builder().create(&staging)?;
        let journal = Self {
            db,
            journal_epoch: epoch,
        };
        journal.write_image(&image)?;
        journal.install_base(&image)?;
        journal.commit_control(crate::ledger::ControlRecord {
            readiness: "HALTED".into(), writable: false, connected: Some(false),
            reason: "backup restored into a new epoch; external reconciliation and old-instance isolation required".into(),
        })?;
        journal.verify_recovery()?;
        drop(journal);
        // A same-filesystem hard link publishes without overwriting a competing target
        fs::hard_link(&staging, target).map_err(|e| file_error(&e))?;
        fs::remove_file(staging).map_err(|e| file_error(&e))?;
        Self::open(target)
    }

    fn write_image(&self, image: &Image) -> Result<(), JournalError> {
        let mut write = self.db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        macro_rules! converted {
            (Number, $value:expr) => {
                *$value
            };
            (Bytes, $value:expr) => {
                $value.as_slice()
            };
            (Text, $value:expr) => {
                $value.as_str()
            };
        }
        macro_rules! populate {
            ($definition:expr, $name:expr, $key_kind:ident, $value_kind:ident) => {{
                let mut table = write.open_table($definition)?;
                table.retain(|_, _| false)?;
                for row in image.rows.iter().filter(|row| row.table == $name) {
                    let (RowKey::$key_kind(key), RowValue::$value_kind(value)) =
                        (&row.key, &row.value)
                    else {
                        return Err(invalid("invalid recovery table row types"));
                    };
                    table.insert(converted!($key_kind, key), converted!($value_kind, value))?;
                }
            }};
        }
        populate!(JOURNAL, "journal", Number, Bytes);
        populate!(ORDERS, "orders", Number, Bytes);
        populate!(RESULTS, "results", Number, Bytes);
        populate!(OUTCOMES, "outcomes", Number, Bytes);
        populate!(EVENT_VIEWS, "event_views", Number, Bytes);
        populate!(CONTROL, "control", Number, Bytes);
        populate!(INPUTS, "inputs", Text, Bytes);
        populate!(BALANCES, "balances", Text, Bytes);
        populate!(EXECUTIONS, "executions", Text, Bytes);
        populate!(SECRETS, "secrets", Text, Bytes);
        populate!(REQUEST_INDEX, "index", Bytes, Number);
        populate!(META, "meta", Number, Number);
        write.commit()?;
        Ok(())
    }
}

fn fresh_epoch(old: u64) -> u64 {
    loop {
        let bytes = *blake3::hash(UUID4::new().to_string().as_bytes()).as_bytes();
        let value = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"));
        if value != 0 && value != old {
            return value;
        }
    }
}

fn file_error(e: &std::io::Error) -> JournalError {
    JournalError::Storage(e.to_string())
}

fn publish_image(
    directory: &Path,
    image: &Image,
    identity: RecoveryIdentity,
) -> Result<PathBuf, JournalError> {
    fs::create_dir_all(directory).map_err(|e| file_error(&e))?;
    let id = UUID4::new().to_string();
    let blob_name = format!("{id}.blob.json");
    let bytes = serde_json::to_vec(image)?;
    if bytes.len() as u64 > MAX_BLOB_BYTES {
        return Err(invalid("recovery blob exceeds size limit"));
    }
    let mut blob = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(&blob_name))
        .map_err(|e| file_error(&e))?;
    blob.write_all(&bytes).map_err(|e| file_error(&e))?;
    blob.sync_all().map_err(|e| file_error(&e))?;
    drop(blob);
    let manifest = RecoveryManifest {
        format_version: 2,
        journal_epoch: image.number(META_EPOCH),
        cursor: image.number(META_LAST_SEQ),
        blob: blob_name,
        blob_bytes: bytes.len() as u64,
        blake3: blake3::hash(&bytes).to_hex().to_string(),
        identity,
    };
    let pending = directory.join(format!("{id}.pending"));
    let published = directory.join(format!("{id}.manifest.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| file_error(&e))?;
    file.write_all(&serde_json::to_vec_pretty(&manifest)?)
        .map_err(|e| file_error(&e))?;
    file.sync_all().map_err(|e| file_error(&e))?;
    drop(file);
    fs::rename(pending, &published).map_err(|e| file_error(&e))?;
    #[cfg(unix)]
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|e| file_error(&e))?;
    Ok(published)
}

fn load_image(path: &Path) -> Result<(RecoveryManifest, Image), JournalError> {
    if !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".manifest.json"))
    {
        return Err(invalid("only a published recovery manifest can be loaded"));
    }
    if fs::metadata(path).map_err(|e| file_error(&e))?.len() > 16_384 {
        return Err(invalid("recovery manifest too large"));
    }
    let manifest: RecoveryManifest =
        serde_json::from_slice(&fs::read(path).map_err(|e| file_error(&e))?)?;
    if manifest.format_version != 2
        || manifest.blob_bytes > MAX_BLOB_BYTES
        || Path::new(&manifest.blob).components().count() != 1
        || !manifest.blob.ends_with(".blob.json")
        || manifest.blob.contains(['/', '\\', ':'])
    {
        return Err(invalid("invalid recovery manifest or blob path"));
    }
    let blob = path
        .parent()
        .ok_or_else(|| invalid("manifest has no directory"))?
        .join(&manifest.blob);
    if fs::metadata(&blob).map_err(|e| file_error(&e))?.len() != manifest.blob_bytes {
        return Err(invalid("recovery blob length mismatch"));
    }
    let bytes = fs::read(blob).map_err(|e| file_error(&e))?;
    if blake3::hash(&bytes).to_hex().as_str() != manifest.blake3 {
        return Err(invalid("recovery blob checksum mismatch"));
    }
    let image: Image = serde_json::from_slice(&bytes)?;
    image.validate()?;
    if image.number(META_LAST_SEQ) != manifest.cursor
        || image.number(META_EPOCH) != manifest.journal_epoch
    {
        return Err(invalid("recovery manifest cursor or epoch mismatch"));
    }
    Ok((manifest, image))
}
