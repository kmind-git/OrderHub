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

//! Procedural macros for Nautilus model and serialization types.

#![warn(clippy::pedantic)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]

mod model;
mod support;

use proc_macro::TokenStream;

/// Expands a named-field struct into a Serde-backed custom data type.
///
/// Generates the required Serde derives, `HasTsInit`, `CustomDataTrait`, `Data` conversions,
/// constructor, and display implementation. The struct must contain
/// `ts_event` and `ts_init` fields.
///
#[proc_macro_attribute]
pub fn custom_data(attr: TokenStream, item: TokenStream) -> TokenStream {
    model::expand_custom_data(attr.into(), item.into()).into()
}
