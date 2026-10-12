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

//! Trading domain types used by OrderHub.
//!
//! Identifiers, instruments, accounts, orders, positions, and events, with exact price, quantity,
//! and money types. The node supports a narrower contract than the complete domain model.
//!
//! # Feature Flags
//!
//! - `defi`: Enables retained DeFi domain types.
//! - `ffi`: Enables the retained C interface.
//! - `high-precision`: Uses 128-bit fixed-point price and quantity values.
//! - `test-support`: Enables domain fixtures and builders.

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(test, allow(clippy::large_stack_arrays))]
#![allow(
    clippy::inline_always,
    reason = "hot-path functions use #[inline(always)] intentionally for constant-folding"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match can be clearer than let-else for some patterns"
)]
#![allow(
    clippy::float_cmp,
    reason = "numeric domain crate: float equality comparisons are pervasive and intentional"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "serde derives on types with unsafe methods for FFI are intentional"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "numeric domain crate: casts are fundamental to fixed-point arithmetic and type conversions"
)]
#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "changing pass-by-ref to pass-by-value would break FFI and Python binding signatures"
)]
#![allow(
    clippy::similar_names,
    reason = "domain terminology creates naturally similar names (bid/ask, base/quote)"
)]
#![allow(
    clippy::too_many_lines,
    reason = "trading domain functions with match arms over many variants are complex by nature"
)]
#![allow(
    clippy::match_same_arms,
    reason = "identical match arms are sometimes intentional for documentation and readability"
)]
#![allow(
    clippy::unused_self,
    reason = "PyO3 methods require &self for Python binding even when Rust impl does not use it"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "math formulas (Black-Scholes, Greeks) use standard single-character variable names"
)]
#![allow(
    clippy::large_types_passed_by_value,
    reason = "PyO3 methods require owned values extracted from Python objects"
)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]
#![allow(clippy::clone_on_copy)]

// Re-exported so `enum_strum_serde!` can reach serde through `$crate`, which works in a consumer
// that renames its serde dependency or does not depend on it directly.
#[doc(hidden)]
pub use serde as __serde;

pub mod accounts;
pub mod currencies;
pub mod data;
pub mod enums;
pub mod events;
pub mod identifiers;
pub mod instruments;
pub mod macros;
pub mod orderbook;
pub mod orders;
pub mod position;
pub mod reports;
pub mod types;
pub mod venues;

pub(crate) mod expressions;

#[cfg(feature = "ffi")]
pub mod ffi;

#[cfg(any(test, feature = "test-support"))]
pub mod stubs;

#[cfg(feature = "defi")]
pub mod defi;

/// Generates custom-data model behavior.
pub use nautilus_macros::custom_data;
