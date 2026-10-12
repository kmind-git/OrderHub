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

//! Order execution engine components used by OrderHub.
//!
//! Order routing, lifecycle events, position updates, and execution reconciliation. The supported
//! node uses the local FIX client; retained engine capabilities do not expand its order contract.
//!
//! # Feature Flags
//!
//! - `high-precision`: Uses 128-bit fixed-point price and quantity values.
//! - `simulation`: Enables deterministic simulation support with `MadSim`.

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::similar_names,
    reason = "execution domain terms such as ts_event/ts_init are intentionally parallel"
)]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "execution math casts between i64/u64/usize/f64 with values bounded by domain ranges"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "execution accessors and constructors are pervasive; #[must_use] noise is not warranted"
)]
#![allow(
    clippy::unused_self,
    reason = "engine and matching operations take &self for method-style organization"
)]
#![allow(
    clippy::large_types_passed_by_value,
    reason = "command and report value types are intentionally moved through dispatch"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "config types deserialize plain field values; unrelated unsafe impls are sound"
)]
#![allow(
    clippy::missing_fields_in_debug,
    reason = "manual Debug impls intentionally omit verbose internal state"
)]
#![allow(
    clippy::struct_excessive_bools,
    reason = "config and snapshot structs mirror existing Python configuration surfaces"
)]
#![allow(
    clippy::too_many_lines,
    reason = "engine and matching dispatch functions exceed the default threshold by design"
)]
#![allow(
    clippy::inline_always,
    reason = "hot-path matching engine functions are intentionally always inlined"
)]
#![allow(
    clippy::match_same_arms,
    reason = "explicit per-variant arms document order/event dispatch even when bodies coincide"
)]
#![allow(
    clippy::match_wildcard_for_single_variants,
    reason = "wildcard arms guard against future enum variants in command dispatch"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match-with-early-return is consistent with surrounding engine and reconciliation code"
)]
#![allow(
    clippy::single_match_else,
    reason = "two-arm matches are consistent with surrounding command and event dispatch"
)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]
#![cfg_attr(
    test,
    allow(
        clippy::default_trait_access,
        clippy::float_cmp,
        clippy::should_panic_without_expect,
        clippy::unreadable_literal,
        clippy::used_underscore_binding,
        reason = "execution tests assert exact float outputs and use loose patterns for fixture setup"
    )
)]

pub mod client;
pub mod engine;
pub mod matching_core;
pub mod matching_engine;
pub mod models;
pub mod order_emulator;
pub mod order_manager;
pub mod protection;
pub mod reconciliation;
pub mod trailing;
