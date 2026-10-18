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

//! Selects exactly one source of account and market inputs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputMode {
    Disabled,
    Stdin,
    TestAccount,
}

impl InputMode {
    pub(super) fn parse(stdin: bool, test_account: Option<&str>) -> Result<Self, &'static str> {
        let test_account = match test_account {
            None | Some("0" | "false") => false,
            Some("1" | "true") => true,
            Some(_) => return Err("ORDERHUB_TEST_ACCOUNT must be 0, 1, false or true"),
        };
        match (stdin, test_account) {
            (true, true) => {
                Err("ORDERHUB_TEST_ACCOUNT and ORDERHUB_INPUT_STDIN are mutually exclusive")
            }
            (true, false) => Ok(Self::Stdin),
            (false, true) => Ok(Self::TestAccount),
            (false, false) => Ok(Self::Disabled),
        }
    }

    #[must_use]
    pub(super) const fn source_id(self) -> &'static str {
        match self {
            Self::TestAccount => orderhub_gateway::inputs::TEST_ACCOUNT_SOURCE,
            Self::Disabled | Self::Stdin => "local-fixture-v1",
        }
    }

    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Stdin => "stdin",
            Self::TestAccount => orderhub_gateway::inputs::TEST_ACCOUNT_SOURCE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InputMode;

    #[test]
    fn test_account_requires_explicit_activation_and_excludes_stdin() {
        assert_eq!(InputMode::parse(false, None), Ok(InputMode::Disabled));
        assert_eq!(InputMode::parse(true, None), Ok(InputMode::Stdin));
        for value in ["1", "true"] {
            assert_eq!(
                InputMode::parse(false, Some(value)),
                Ok(InputMode::TestAccount)
            );
            assert!(InputMode::parse(true, Some(value)).is_err());
        }
        for value in ["0", "false"] {
            assert_eq!(
                InputMode::parse(false, Some(value)),
                Ok(InputMode::Disabled)
            );
            assert_eq!(InputMode::parse(true, Some(value)), Ok(InputMode::Stdin));
        }
        for value in ["", "yes", "2"] {
            assert!(InputMode::parse(false, Some(value)).is_err());
        }
        assert_ne!(InputMode::Stdin.as_str(), InputMode::TestAccount.as_str());
    }
}
