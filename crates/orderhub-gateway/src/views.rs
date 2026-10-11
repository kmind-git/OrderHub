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

//! Bounded snapshot leases and identity-bound signed continuation tokens.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tonic::Status;

use crate::auth::SubmitterCredentials;
use crate::journal::CommittedSnapshot;

pub(crate) const PAGE_MAXIMUM: usize = 1_000;
pub(crate) const SNAPSHOT_MAXIMUM: usize = 4_096;
const LEASE_MAXIMUM: usize = 16;
const LEASE_BYTE_MAXIMUM: usize = 8 * 1024 * 1024;
const LEASE_TTL_MS: u64 = 60_000;
const STREAM_TOKEN_TTL_MS: u64 = 3_600_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ViewToken {
    pub epoch: u64,
    pub submitter: String,
    pub scope: String,
    pub cursor: u64,
    pub lease: String,
    pub offset: usize,
    pub expires_ms: u64,
}

#[derive(Debug)]
pub(crate) struct SnapshotLease {
    pub data: CommittedSnapshot,
    pub credentials: SubmitterCredentials,
    pub expires_ms: u64,
    pub bytes: usize,
}

#[derive(Debug, Default)]
pub(crate) struct SnapshotStore {
    pub leases: HashMap<String, SnapshotLease>,
}

impl SnapshotStore {
    pub(crate) fn insert(
        &mut self,
        data: CommittedSnapshot,
        credentials: SubmitterCredentials,
    ) -> Result<String, Status> {
        self.leases.retain(|_, lease| lease.expires_ms > now_ms());
        let bytes = serde_json::to_vec(&(&data.orders, &data.inputs))
            .map_err(|e| Status::internal(e.to_string()))?
            .len()
            + data
                .positions
                .iter()
                .map(|position| position.quantity.len() + position.cost.len() + 256)
                .sum::<usize>()
            + data
                .accounts
                .iter()
                .map(|account| account.reserved_cash.len() + account.spent_cash.len() + 256)
                .sum::<usize>()
            + credentials
                .strategies
                .iter()
                .map(String::len)
                .sum::<usize>()
            + 1024;
        if self.leases.len() >= LEASE_MAXIMUM
            || bytes > LEASE_BYTE_MAXIMUM
            || self.leases.values().map(|lease| lease.bytes).sum::<usize>() + bytes
                > LEASE_BYTE_MAXIMUM
        {
            return Err(Status::resource_exhausted("snapshot lease limit reached"));
        }
        let id = nautilus_core::UUID4::new().to_string();
        self.leases.insert(
            id.clone(),
            SnapshotLease {
                data,
                credentials,
                expires_ms: now_ms() + LEASE_TTL_MS,
                bytes,
            },
        );
        Ok(id)
    }
}

pub(crate) fn scope(credentials: &SubmitterCredentials) -> String {
    let mut strategies = credentials.strategies.clone();
    strategies.sort();
    blake3::hash(
        serde_json::to_string(&strategies)
            .expect("scope serialization")
            .as_bytes(),
    )
    .to_hex()
    .to_string()
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub(crate) fn issue(
    key: &[u8; 32],
    epoch: u64,
    credentials: &SubmitterCredentials,
    cursor: u64,
    lease: String,
    offset: usize,
    expires_ms: Option<u64>,
) -> String {
    let token = ViewToken {
        epoch,
        submitter: credentials.submitter_id.clone(),
        scope: scope(credentials),
        cursor,
        lease,
        offset,
        expires_ms: expires_ms.unwrap_or_else(|| now_ms() + STREAM_TOKEN_TTL_MS),
    };
    let payload = serde_json::to_vec(&token).expect("token serialization");
    let encoded = payload
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{encoded}.{}", blake3::keyed_hash(key, &payload).to_hex())
}

pub(crate) fn verify(
    key: &[u8; 32],
    encoded: &str,
    epoch: u64,
    credentials: &SubmitterCredentials,
) -> Result<ViewToken, Status> {
    if encoded.len() > 2_048 {
        return Err(Status::invalid_argument("token exceeds size limit"));
    }
    let (payload, signature) = encoded
        .split_once('.')
        .ok_or_else(|| Status::invalid_argument("invalid token"))?;
    if payload.len() % 2 != 0 || signature.len() != 64 || !payload.is_ascii() {
        return Err(Status::invalid_argument("invalid token encoding"));
    }
    let payload: Vec<u8> = (0..payload.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&payload[offset..offset + 2], 16))
        .collect::<Result<_, _>>()
        .map_err(|_| Status::invalid_argument("invalid token encoding"))?;
    let expected = blake3::keyed_hash(key, &payload).to_hex();
    let different = signature
        .bytes()
        .zip(expected.as_str().bytes())
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b));
    if different != 0 {
        return Err(Status::permission_denied("invalid token signature"));
    }
    let token: ViewToken = serde_json::from_slice(&payload)
        .map_err(|_| Status::invalid_argument("invalid token payload"))?;
    if token.epoch != epoch || token.expires_ms <= now_ms() {
        return Err(Status::out_of_range("RESYNC_REQUIRED"));
    }
    if token.submitter != credentials.submitter_id || token.scope != scope(credentials) {
        return Err(Status::permission_denied("token authorization changed"));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> SubmitterCredentials {
        SubmitterCredentials {
            token: "test".into(),
            submitter_id: "owner".into(),
            strategies: vec!["STRAT-S001".into()],
        }
    }

    fn empty_snapshot() -> CommittedSnapshot {
        CommittedSnapshot {
            control: None,
            cursor: 0,
            orders: vec![],
            inputs: vec![],
            positions: vec![],
            accounts: vec![],
        }
    }

    #[test]
    fn test_signed_tokens_reject_expiry_epoch_and_changed_permission() {
        let key = [23; 32];
        let owner = credentials();
        let good = issue(&key, 1, &owner, 42, String::new(), 0, None);
        assert_eq!(verify(&key, &good, 1, &owner).unwrap().cursor, 42);
        assert_eq!(
            verify(&key, &good, 2, &owner).unwrap_err().code(),
            tonic::Code::OutOfRange
        );
        let mut revoked = owner.clone();
        revoked.strategies = vec!["STRAT-S002".into()];
        assert_eq!(
            verify(&key, &good, 1, &revoked).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        let expired = issue(
            &key,
            1,
            &owner,
            42,
            String::new(),
            0,
            Some(now_ms().saturating_sub(1)),
        );
        assert_eq!(
            verify(&key, &expired, 1, &owner).unwrap_err().code(),
            tonic::Code::OutOfRange
        );
    }

    #[test]
    fn test_snapshot_leases_reject_capacity_and_reclaim_expired_data() {
        let mut store = SnapshotStore::default();
        for _ in 0..LEASE_MAXIMUM {
            store.insert(empty_snapshot(), credentials()).unwrap();
        }
        assert_eq!(
            store
                .insert(empty_snapshot(), credentials())
                .unwrap_err()
                .code(),
            tonic::Code::ResourceExhausted
        );
        for lease in store.leases.values_mut() {
            lease.expires_ms = 0;
        }
        store.insert(empty_snapshot(), credentials()).unwrap();
        assert_eq!(store.leases.len(), 1);
    }
}
