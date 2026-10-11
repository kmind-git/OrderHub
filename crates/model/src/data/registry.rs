// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Registry for custom data JSON serialization and deserialization.
//!
//! The optional offline catalog converter handles supported built-in types separately.
//! The registry only stores type name -> callbacks for lookup; each type provides
//! its own deserialize/encode/decode via the trait or registration.

use std::sync::Arc;

use dashmap::{DashMap, mapref::entry::Entry};
use nautilus_core::Params;

use crate::data::{CustomData, CustomDataTrait, Data, DataType};

pub type JsonDeserializer =
    Box<dyn Fn(serde_json::Value) -> Result<Arc<dyn CustomDataTrait>, anyhow::Error> + Send + Sync>;

struct Registries {
    json: DashMap<String, JsonDeserializer>,
}

fn registries() -> &'static Registries {
    static REGISTRIES: std::sync::OnceLock<Registries> = std::sync::OnceLock::new();
    REGISTRIES.get_or_init(|| Registries {
        json: DashMap::new(),
    })
}

/// Registers a JSON deserializer for the given custom data type name.
/// When `Data::deserialize` sees this type name, it will call this function.
///
/// # Errors
/// Returns an error if the type is already registered.
pub fn register_json_deserializer(
    type_name: &str,
    deserializer: JsonDeserializer,
) -> Result<(), anyhow::Error> {
    let reg = registries();
    match reg.json.entry(type_name.to_string()) {
        Entry::Occupied(_) => {
            anyhow::bail!("Custom data type \"{type_name}\" is already registered for JSON");
        }
        Entry::Vacant(v) => {
            v.insert(deserializer);
            Ok(())
        }
    }
}

/// Registers a JSON deserializer for the given custom data type name if not already registered.
/// If the type is already registered, returns `Ok(())` without overwriting (idempotent).
/// Use this where repeated registration can occur (e.g. module init).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
pub fn ensure_json_deserializer_registered(
    type_name: &str,
    deserializer: JsonDeserializer,
) -> Result<(), anyhow::Error> {
    let reg = registries();
    reg.json
        .entry(type_name.to_string())
        .or_insert_with(|| deserializer);
    Ok(())
}

/// Parses a "`data_type`" JSON object into `DataType` (`type_name`, metadata, identifier).
fn parse_data_type_from_value(value: &serde_json::Value) -> Option<DataType> {
    let obj = value.get("data_type")?.as_object()?;
    let type_name = obj.get("type_name")?.as_str()?;
    let metadata = obj.get("metadata").and_then(|m| {
        if m.is_null() {
            None
        } else {
            let p: Params = serde_json::from_value(m.clone()).ok()?;
            if p.is_empty() { None } else { Some(p) }
        }
    });
    let identifier = obj
        .get("identifier")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some(DataType::new(type_name, metadata, identifier))
}

/// Parses the canonical `CustomData` JSON envelope `{ type, data_type, payload }` and returns
/// the payload value to pass to the registered type deserializer. Does not depend on
/// user payload field names.
fn parse_envelope_payload(value: &serde_json::Value) -> Result<serde_json::Value, anyhow::Error> {
    let payload = value
        .get("payload")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("CustomData JSON missing 'payload' field"))?;
    Ok(payload)
}

/// Looks up and runs the JSON deserializer for the given type name.
/// Returns `None` if the type is not registered.
///
/// # Errors
/// Returns an error if the deserializer fails.
pub fn deserialize_custom_from_json(
    type_name: &str,
    value: &serde_json::Value,
) -> Result<Option<Data>, anyhow::Error> {
    let reg = registries();
    let deserializer_ref = match reg.json.get(type_name) {
        Some(d) => d,
        None => return Ok(None),
    };
    let data_type = parse_data_type_from_value(value);
    let payload = parse_envelope_payload(value)?;
    let arc = deserializer_ref.value()(payload)?;
    let custom = match data_type {
        Some(dt) => CustomData::new(arc, dt),
        None => CustomData::from_arc(arc),
    };
    Ok(Some(Data::Custom(custom)))
}

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::data::{CustomData, custom::register_custom_data_json};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestRegCustomData {
        ts_init: UnixNanos,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct StrictRegCustomData {
        ts_init: UnixNanos,
    }

    impl crate::data::HasTsInit for TestRegCustomData {
        fn ts_init(&self) -> UnixNanos {
            self.ts_init
        }
    }

    impl crate::data::custom::CustomDataTrait for TestRegCustomData {
        fn type_name(&self) -> &'static str {
            "TestRegCustomData"
        }
        fn type_name_static() -> &'static str {
            "TestRegCustomData"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn ts_event(&self) -> nautilus_core::UnixNanos {
            self.ts_init
        }
        fn to_json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
        fn clone_arc(&self) -> Arc<dyn crate::data::CustomDataTrait> {
            Arc::new(self.clone())
        }
        fn eq_arc(&self, other: &dyn crate::data::CustomDataTrait) -> bool {
            other.as_any().downcast_ref::<Self>() == Some(self)
        }
        fn from_json(
            value: serde_json::Value,
        ) -> anyhow::Result<Arc<dyn crate::data::CustomDataTrait>> {
            let t: Self = serde_json::from_value(value)?;
            Ok(Arc::new(t))
        }
    }

    impl crate::data::HasTsInit for StrictRegCustomData {
        fn ts_init(&self) -> UnixNanos {
            self.ts_init
        }
    }

    impl crate::data::custom::CustomDataTrait for StrictRegCustomData {
        fn type_name(&self) -> &'static str {
            "StrictRegCustomData"
        }
        fn type_name_static() -> &'static str {
            "StrictRegCustomData"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn ts_event(&self) -> nautilus_core::UnixNanos {
            self.ts_init
        }
        fn to_json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
        fn clone_arc(&self) -> Arc<dyn crate::data::CustomDataTrait> {
            Arc::new(self.clone())
        }
        fn eq_arc(&self, other: &dyn crate::data::CustomDataTrait) -> bool {
            other.as_any().downcast_ref::<Self>() == Some(self)
        }
        fn from_json(
            value: serde_json::Value,
        ) -> anyhow::Result<Arc<dyn crate::data::CustomDataTrait>> {
            let t: Self = serde_json::from_value(value)?;
            Ok(Arc::new(t))
        }
    }

    #[rstest]
    fn json_registry_roundtrip() {
        let _ = register_custom_data_json::<TestRegCustomData>();

        let data = Data::Custom(CustomData::from_arc(Arc::new(TestRegCustomData {
            ts_init: UnixNanos::from(100),
        })));

        let json = serde_json::to_string(&data).unwrap();
        let back: Data = serde_json::from_str(&json).unwrap();

        match (&data, &back) {
            (Data::Custom(a), Data::Custom(b)) => {
                assert_eq!(a.data.type_name(), b.data.type_name());
                assert_eq!(a.data.ts_init(), b.data.ts_init());
            }
            _ => panic!("expected Custom variant"),
        }
    }

    #[rstest]
    fn json_registry_roundtrip_with_deny_unknown_fields() {
        let _ = register_custom_data_json::<StrictRegCustomData>();

        let data = Data::Custom(CustomData::from_arc(Arc::new(StrictRegCustomData {
            ts_init: UnixNanos::from(200),
        })));

        let json = serde_json::to_string(&data).unwrap();
        let back: Data = serde_json::from_str(&json).unwrap();

        match (&data, &back) {
            (Data::Custom(a), Data::Custom(b)) => {
                assert_eq!(a.data.type_name(), b.data.type_name());
                assert_eq!(a.data.ts_init(), b.data.ts_init());
            }
            _ => panic!("expected Custom variant"),
        }
    }

    #[rstest]
    fn data_type_registry_result_hashes_like_equal_values_from_all_routes() {
        fn hash_data_type(data_type: &DataType) -> u64 {
            let mut hasher = DefaultHasher::new();
            data_type.hash(&mut hasher);
            hasher.finish()
        }

        let metadata = serde_json::json!({"key": "value"});
        let constructed = DataType::new(
            "ExampleType",
            Some(serde_json::from_value(metadata.clone()).unwrap()),
            Some("catalog/path".to_string()),
        );
        let persistence_json = serde_json::json!({
            "type_name": constructed.type_name(),
            "metadata": metadata,
            "identifier": constructed.identifier(),
        });
        let persisted = DataType::from_persistence_json(&persistence_json.to_string()).unwrap();
        let registry_envelope = serde_json::json!({"data_type": persistence_json});
        let registered = parse_data_type_from_value(&registry_envelope).unwrap();
        let deserialization_payload = serde_json::json!({
            "type_name": constructed.type_name(),
            "metadata": constructed.metadata(),
            "topic": constructed.topic(),
            "hash": constructed.precomputed_hash() ^ u64::MAX,
            "identifier": constructed.identifier(),
        });
        let deserialized: DataType = serde_json::from_value(deserialization_payload).unwrap();

        // Eq and Hash both observe `topic` alone, so they cannot detect a route that drops or
        // rewrites the other fields. Assert those explicitly as well, or a registry regression
        // that lost `identifier` would still satisfy this test.
        for data_type in [&deserialized, &persisted, &registered] {
            assert_eq!(data_type, &constructed);
            assert_eq!(hash_data_type(data_type), hash_data_type(&constructed));
            assert_eq!(data_type.type_name(), constructed.type_name());
            assert_eq!(data_type.metadata(), constructed.metadata());
            assert_eq!(data_type.identifier(), constructed.identifier());
            assert_eq!(data_type.topic(), constructed.topic());
        }
    }

    #[rstest]
    fn ensure_json_deserializer_registered_is_idempotent() {
        let deserializer: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r1 = ensure_json_deserializer_registered("IdempotentTestJson", deserializer);
        assert!(r1.is_ok(), "first registration should succeed");
        let deserializer2: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r2 = ensure_json_deserializer_registered("IdempotentTestJson", deserializer2);
        assert!(
            r2.is_ok(),
            "second registration with same type_name should succeed (idempotent)"
        );
    }

    #[rstest]
    fn register_json_deserializer_fails_on_duplicate() {
        let deserializer: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r1 = register_json_deserializer("StrictDuplicateTestJson", deserializer);
        assert!(r1.is_ok());
        let deserializer2: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r2 = register_json_deserializer("StrictDuplicateTestJson", deserializer2);
        assert!(r2.is_err());
        let err_msg = r2.unwrap_err().to_string();
        assert!(
            err_msg.contains("already registered"),
            "expected 'already registered' in error, found: {err_msg}"
        );
    }
}
