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

//! Portfolio components used by OrderHub.
//!
//! Account and position tracking, valuation, profit and loss calculations, and currency conversion.
//!
//! # Feature Flags
//!
//! This crate has no optional feature flags.

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
    reason = "portfolio timing and domain terms such as interval_ms/interval_ns are intentionally parallel"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match and if-let early returns are consistent with surrounding portfolio flow code"
)]
#![allow(
    clippy::single_match_else,
    reason = "match can be clearer than if-let-else for some portfolio state transitions"
)]
#![allow(
    clippy::too_many_lines,
    reason = "portfolio calculation and event update flows exceed the default threshold by design"
)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]
#![allow(clippy::clone_on_copy)]

pub mod config;
pub mod manager;
pub mod portfolio;

// Re-exports
pub use portfolio::Portfolio;
