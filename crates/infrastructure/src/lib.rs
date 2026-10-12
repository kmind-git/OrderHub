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

//! Database and messaging backends retained by OrderHub.
//!
//! Redis and PostgreSQL are optional integrations. Their service behavior requires separate
//! activation validation; the current node rejects their activation flags. TLS consumers share
//! the approved provider when its feature is enabled.
//!
//! # Feature Flags
//!
//! - `postgres-tls`: Enables PostgreSQL TLS through the custom SQLx provider.
//! - `postgres`: Enables SQLx PostgreSQL models and Cache database support.
//! - `redis`: Enables Redis Cache and message bus implementations (default).
//! - `tls`: Enables the shared TLS provider and CPU capability gate.

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
    reason = "domain terms such as trader_id/trade_id and price_precision/size_precision are intentionally parallel"
)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]

#[cfg(feature = "redis")]
pub mod redis;

#[cfg(feature = "postgres")]
pub mod sql;

#[cfg(feature = "tls")]
pub mod tls;
