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

//! JSON-only counterpart of the persistence catalog fixture, with the same wire fields.

use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    custom_data,
    data::{CustomData, DataType, ensure_custom_data_json_registered},
    identifiers::InstrumentId,
};
use serde_json::json;

#[custom_data]
pub(crate) struct RustTestCustomData {
    pub instrument_id: InstrumentId,
    pub value: f64,
    pub flag: bool,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[test]
fn test_custom_data_json_preserves_database_envelope() {
    ensure_custom_data_json_registered::<RustTestCustomData>().unwrap();
    let metadata = indexmap::indexmap! {
        "a".to_string() => json!("1"),
        "b".to_string() => json!("2"),
    };
    let data_type = DataType::new(
        "RustTestCustomData",
        Some(Params::from_index_map(metadata)),
        Some("RUST.TEST".to_string()),
    );
    let data = CustomData::new(
        std::sync::Arc::new(RustTestCustomData {
            instrument_id: InstrumentId::from("RUST.TEST"),
            value: 42.0,
            flag: true,
            ts_event: UnixNanos::from(10),
            ts_init: UnixNanos::from(20),
        }),
        data_type,
    );
    let value = serde_json::to_value(&data).unwrap();
    assert_eq!(value["type"], "RustTestCustomData");
    assert_eq!(value["data_type"]["type_name"], "RustTestCustomData");
    assert_eq!(value["data_type"]["identifier"], "RUST.TEST");
    assert_eq!(value["data_type"]["metadata"], json!({"a": "1", "b": "2"}));
    assert_eq!(
        value["payload"],
        json!({
            "instrument_id": "RUST.TEST",
            "value": 42.0,
            "flag": true,
            "ts_event": 10,
            "ts_init": 20,
        }),
    );
    let bytes = serde_json::to_vec(&value).unwrap();
    assert_eq!(CustomData::from_json_bytes(&bytes).unwrap(), data);
}
