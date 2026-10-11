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

use std::{any::Any, fmt::Debug, sync::Arc};

use nautilus_core::UnixNanos;

use serde::{Serialize, Serializer};

use crate::data::{
    Data, DataType, HasTsInit,
    registry::{ensure_json_deserializer_registered, register_json_deserializer},
};

/// Trait for typed custom data that can be used within the Nautilus domain model.
pub trait CustomDataTrait: HasTsInit + Send + Sync + Debug {
    /// Returns the type name for the custom data.
    fn type_name(&self) -> &'static str;

    /// Returns the data as a `dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns the event timestamp (when the data occurred).
    fn ts_event(&self) -> UnixNanos;

    /// Serializes the custom data to a JSON string.
    ///
    /// # Errors
    /// Returns an error if JSON serialization fails.
    fn to_json(&self) -> anyhow::Result<String>;

    /// Python-facing JSON serialization. Default implementation forwards to `to_json`.
    /// Override if a different behavior is needed for the Python API.
    ///
    /// # Errors
    /// Returns an error if JSON serialization fails.
    fn to_json_py(&self) -> anyhow::Result<String> {
        self.to_json()
    }

    /// Returns a cloned Arc of the custom data.
    fn clone_arc(&self) -> Arc<dyn CustomDataTrait>;

    /// Returns whether the custom data is equal to another.
    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool;

    /// Returns the type name used in serialized form (e.g. in the `"type"` field).
    #[must_use]
    fn type_name_static() -> &'static str
    where
        Self: Sized,
    {
        std::any::type_name::<Self>()
    }

    /// Deserializes from a JSON value into an Arc'd trait object.
    ///
    /// # Errors
    /// Returns an error if JSON deserialization fails.
    fn from_json(_value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>>
    where
        Self: Sized,
    {
        anyhow::bail!(
            "from_json not implemented for {}",
            std::any::type_name::<Self>()
        )
    }
}

/// Registers a custom data type for JSON deserialization. When `Data::deserialize`
/// sees the type name returned by `T::type_name_static()`, it will call `T::from_json`.
///
/// # Errors
/// Returns an error if the type is already registered.
pub fn register_custom_data_json<T: CustomDataTrait + Sized>() -> anyhow::Result<()> {
    let type_name = T::type_name_static();
    register_json_deserializer(type_name, Box::new(|value| T::from_json(value)))
}

/// Registers a custom data type for JSON deserialization if not already registered.
/// Idempotent: safe to call multiple times for the same type (e.g. module init).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
pub fn ensure_custom_data_json_registered<T: CustomDataTrait + Sized>() -> anyhow::Result<()> {
    let type_name = T::type_name_static();
    ensure_json_deserializer_registered(type_name, Box::new(|value| T::from_json(value)))
}

/// A wrapper for custom data including its data type.
///
/// The `data` field holds an [`Arc`] to a [`CustomDataTrait`] implementation,
/// enabling cheap cloning when passing to Python (Arc clone is O(1)).
/// Custom data is always Rust-defined (optionally with PyO3 bindings).
#[derive(Clone, Debug)]
pub struct CustomData {
    /// The actual data object implementing [`CustomDataTrait`].
    pub data: Arc<dyn CustomDataTrait>,
    /// The data type metadata.
    pub data_type: DataType,
}

impl CustomData {
    /// Creates a new [`CustomData`] instance from an [`Arc`]'d [`CustomDataTrait`],
    /// deriving the data type from the inner type name.
    pub fn from_arc(arc: Arc<dyn CustomDataTrait>) -> Self {
        let data_type = DataType::new(arc.type_name(), None, None);
        Self {
            data: arc,
            data_type,
        }
    }

    /// Creates a new [`CustomData`] instance with explicit data type metadata.
    ///
    /// Use this when the data type must come from external metadata (e.g. Parquet),
    /// rather than being derived from the inner type name.
    pub fn new(data: Arc<dyn CustomDataTrait>, data_type: DataType) -> Self {
        Self { data, data_type }
    }
}

impl PartialEq for CustomData {
    fn eq(&self, other: &Self) -> bool {
        self.data.eq_arc(other.data.as_ref()) && self.data_type == other.data_type
    }
}

impl HasTsInit for CustomData {
    fn ts_init(&self) -> UnixNanos {
        self.data.ts_init()
    }
}

pub(crate) fn parse_custom_data_from_json_bytes(
    bytes: &[u8],
) -> Result<CustomData, serde_json::Error> {
    let data: Data = serde_json::from_slice(bytes)?;
    match data {
        Data::Custom(custom) => Ok(custom),
        _ => Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON does not represent CustomData",
        ))),
    }
}

impl CustomData {
    /// Deserializes `CustomData` from JSON bytes (full `CustomData` format with type and `data_type`).
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes are not valid JSON or do not represent `CustomData`.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        parse_custom_data_from_json_bytes(bytes)
    }
}

/// Canonical JSON envelope for `CustomData`. All serialized `CustomData` uses this shape so
/// deserialization can extract the payload without depending on user payload field names.
struct CustomDataEnvelope {
    type_name: String,
    data_type: serde_json::Value,
    payload: serde_json::Value,
}

impl Serialize for CustomDataEnvelope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CustomDataEnvelope", 3)?;
        state.serialize_field("type", &self.type_name)?;
        state.serialize_field("data_type", &self.data_type)?;
        state.serialize_field("payload", &self.payload)?;
        state.end()
    }
}

impl CustomData {
    fn to_envelope_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        let json = self.data.to_json().map_err(|e| {
            serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let payload: serde_json::Value = serde_json::from_str(&json)?;
        let metadata_value = self.data_type.metadata().map_or(
            serde_json::Value::Object(serde_json::Map::new()),
            |m| {
                serde_json::to_value(m).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
            },
        );
        let mut data_type_obj = serde_json::Map::new();
        data_type_obj.insert(
            "type_name".to_string(),
            serde_json::Value::String(self.data_type.type_name().to_string()),
        );
        data_type_obj.insert("metadata".to_string(), metadata_value);

        if let Some(id) = self.data_type.identifier() {
            data_type_obj.insert(
                "identifier".to_string(),
                serde_json::Value::String(id.to_string()),
            );
        }

        let envelope = CustomDataEnvelope {
            type_name: self.data.type_name().to_string(),
            data_type: serde_json::Value::Object(data_type_obj),
            payload,
        };
        serde_json::to_value(envelope)
    }
}

impl Serialize for CustomData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = self
            .to_envelope_json_value()
            .map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{Params, UnixNanos};
    use rstest::rstest;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::{data::HasTsInit, identifiers::InstrumentId};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestCustomData {
        ts_init: UnixNanos,
        instrument_id: InstrumentId,
    }

    impl HasTsInit for TestCustomData {
        fn ts_init(&self) -> UnixNanos {
            self.ts_init
        }
    }

    impl CustomDataTrait for TestCustomData {
        fn type_name(&self) -> &'static str {
            "TestCustomData"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn ts_event(&self) -> UnixNanos {
            self.ts_init
        }
        fn to_json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
        fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
            Arc::new(self.clone())
        }
        fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
            if let Some(other) = other.as_any().downcast_ref::<Self>() {
                self == other
            } else {
                false
            }
        }

        fn type_name_static() -> &'static str {
            "TestCustomData"
        }

        fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
            let parsed: Self = serde_json::from_value(value)?;
            Ok(Arc::new(parsed))
        }
    }

    #[rstest]
    fn test_custom_data_json_roundtrip() {
        register_custom_data_json::<TestCustomData>()
            .expect("TestCustomData must register for JSON roundtrip test");

        let instrument_id = InstrumentId::from("TEST.SIM");
        let metadata = Some(
            serde_json::from_value::<Params>(json!({"key1": "value1", "key2": "value2"})).unwrap(),
        );
        let inner = TestCustomData {
            ts_init: UnixNanos::from(100),
            instrument_id,
        };
        let data_type = DataType::new("TestCustomData", metadata, Some(instrument_id.to_string()));
        let original = CustomData::new(Arc::new(inner), data_type);

        let json_bytes = serde_json::to_vec(&original).unwrap();
        let roundtripped = CustomData::from_json_bytes(&json_bytes).unwrap();

        assert_eq!(
            roundtripped.data_type.type_name(),
            original.data_type.type_name()
        );
        assert_eq!(
            roundtripped.data_type.metadata(),
            original.data_type.metadata()
        );
        assert_eq!(
            roundtripped.data_type.identifier(),
            original.data_type.identifier()
        );
        let orig_inner = original
            .data
            .as_any()
            .downcast_ref::<TestCustomData>()
            .unwrap();
        let rt_inner = roundtripped
            .data
            .as_any()
            .downcast_ref::<TestCustomData>()
            .unwrap();
        assert_eq!(orig_inner, rt_inner);
    }

    #[rstest]
    fn test_custom_data_wrapper() {
        let instrument_id = InstrumentId::from("TEST.SIM");
        let data = TestCustomData {
            ts_init: UnixNanos::from(100),
            instrument_id,
        };
        let data_type = DataType::new("TestCustomData", None, Some(instrument_id.to_string()));
        let custom_data = CustomData::new(Arc::new(data), data_type);

        assert_eq!(custom_data.data.ts_init(), UnixNanos::from(100));
        assert_eq!(Data::Custom(custom_data).instrument_id(), instrument_id);
    }
}
