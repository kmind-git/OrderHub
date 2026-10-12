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

//! Core constants.

/// The application name, exposed through a retained compatibility identifier.
pub static NAUTILUS_TRADER: &str = "OrderHub";

/// The `nautilus-core` crate version string embedded at compile time.
pub static NAUTILUS_VERSION_CORE: &str = env!("CARGO_PKG_VERSION");

/// The OrderHub version string selected for the compiled application.
pub static NAUTILUS_VERSION: &str = NAUTILUS_VERSION_CORE;

/// The OrderHub User-Agent string including the current version at compile time.
pub static NAUTILUS_USER_AGENT: &str = concat!("OrderHub/", env!("CARGO_PKG_VERSION"));

/// Prefix for log messages outside the main logging subsystem.
pub static NAUTILUS_PREFIX: &str = "[ORDERHUB]";

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_nautilus_versions_rust() {
        assert_eq!(NAUTILUS_VERSION_CORE, env!("CARGO_PKG_VERSION"));
        assert_eq!(NAUTILUS_VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            NAUTILUS_USER_AGENT,
            concat!("OrderHub/", env!("CARGO_PKG_VERSION")),
        );
    }
}
