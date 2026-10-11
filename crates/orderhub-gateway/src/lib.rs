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

//! OrderHub admission, exact business ledger, and committed gRPC views.
//!
//! The bridge preserves the native Cache -> RiskEngine -> ExecutionEngine path.
//! A redb transaction commits the request result, order intent, reservation,
//! pending dispatch, indexes, and event view before acknowledgment or dispatch.
//! Execution facts commit before native publication, with stable trade identity
//! and complete exact economics. Persistence failures close the new-risk gate.
//!
//! Queries and bounded replay read committed data only; signed continuation
//! tokens bind epoch and current credentials. Database services remain inactive.
//! Process recovery, execution reconciliation, backups, and the local FIX
//! end-to-end acceptance are scheduled for P6. Existing journals keep the node
//! HALTED until that execution-reconciliation gate is implemented and verified.
pub mod auth;
pub mod bridge;
pub mod exec_wiring;
pub mod grpc;
pub mod inputs;
pub mod journal;
pub mod ledger;
pub mod local_client;
pub mod local_exchange;
pub mod quota;
pub mod readiness;
pub mod recovery;
pub mod transport;
pub mod worker;

mod views;
