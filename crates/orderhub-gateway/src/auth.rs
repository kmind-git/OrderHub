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

//! Per-submitter credential mapping (M2).
//!
//! Server-side mapping from bearer tokens to submitter identities and their
//! authorized strategy set, per the risk contract: `strategy_id` is an
//! attribution label validated against the credential, never a credential
//! itself. Requests referencing strategies outside the credential's
//! authorization are rejected without disclosing other submitters' data.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One submitter's credentials and authorization scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitterCredentials {
    /// Shared bearer token for this submitter.
    pub token: String,
    /// Server-side submitter identity.
    pub submitter_id: String,
    /// Strategy IDs this submitter may submit and query for.
    pub strategies: Vec<String>,
}

/// Registry of all authorized submitters, keyed by token.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SubmitterRegistry {
    submitters: Vec<SubmitterCredentials>,
    #[serde(skip)]
    by_token: HashMap<String, usize>,
}

impl SubmitterRegistry {
    /// Validates credentials and rebuilds the lookup index after configuration decoding.
    pub fn try_new(submitters: Vec<SubmitterCredentials>) -> Result<Self, String> {
        if submitters.is_empty() || submitters.len() > 128 {
            return Err("credential count must be 1..=128".into());
        }
        let mut by_token = HashMap::new();
        let mut owners = HashMap::new();
        for (index, credentials) in submitters.iter().enumerate() {
            if credentials.token.is_empty()
                || credentials.token.len() > 256
                || credentials.submitter_id.is_empty()
                || credentials.submitter_id.len() > 64
                || credentials.submitter_id.chars().any(char::is_control)
                || credentials.strategies.is_empty()
                || credentials.strategies.len() > 128
            {
                return Err("invalid credential field size".into());
            }
            if by_token.insert(credentials.token.clone(), index).is_some() {
                return Err("duplicate submitter token".into());
            }
            for strategy in &credentials.strategies {
                if strategy.is_empty()
                    || strategy.len() > 64
                    || strategy.chars().any(char::is_control)
                {
                    return Err("invalid strategy identity".into());
                }
                if owners
                    .insert(strategy.clone(), credentials.submitter_id.clone())
                    .is_some_and(|owner| owner != credentials.submitter_id)
                {
                    return Err("strategy assigned to multiple submitters".into());
                }
            }
        }
        Ok(Self {
            submitters,
            by_token,
        })
    }

    pub(crate) fn submitter_ids(&self) -> Vec<String> {
        self.submitters
            .iter()
            .map(|credentials| credentials.submitter_id.clone())
            .collect()
    }
    /// Creates a registry from a credential list.
    ///
    /// # Panics
    ///
    /// Panics on duplicate tokens: they are a configuration error and must
    /// fail fast at startup rather than silently granting another identity.
    #[must_use]
    pub fn new(submitters: Vec<SubmitterCredentials>) -> Self {
        Self::try_new(submitters).expect("invalid submitter credentials")
    }

    /// Authenticates a raw bearer token to its submitter credentials.
    #[must_use]
    pub fn authenticate(&self, token: &str) -> Option<&SubmitterCredentials> {
        self.by_token
            .get(token)
            .and_then(|&index| self.submitters.get(index))
    }

    /// Authenticates a bearer-token header value (`Bearer <token>`).
    #[must_use]
    pub fn authenticate_header(&self, header: &str) -> Option<&SubmitterCredentials> {
        header
            .strip_prefix("Bearer ")
            .and_then(|token| self.authenticate(token))
    }

    /// Whether `strategy_id` is within the credential's authorization set.
    #[must_use]
    pub fn is_authorized(credentials: &SubmitterCredentials, strategy_id: &str) -> bool {
        credentials.strategies.iter().any(|s| s == strategy_id)
    }
}

impl<'de> Deserialize<'de> for SubmitterRegistry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Configuration {
            submitters: Vec<SubmitterCredentials>,
        }
        let configuration = Configuration::deserialize(deserializer)?;
        Self::try_new(configuration.submitters).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_credentials_rebuild_token_index_and_reject_shared_ownership() {
        let registry: SubmitterRegistry = serde_json::from_str(r#"{"submitters":[{"token":"test","submitter_id":"owner","strategies":["STRAT-S001"]}]}"#).unwrap();
        assert_eq!(registry.authenticate("test").unwrap().submitter_id, "owner");
        assert!(serde_json::from_str::<SubmitterRegistry>(r#"{"submitters":[{"token":"a","submitter_id":"one","strategies":["STRAT-S001"]},{"token":"b","submitter_id":"two","strategies":["STRAT-S001"]}]}"#).is_err());
    }
}
