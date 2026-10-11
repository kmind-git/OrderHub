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

//! Shared `syn` parsing and type inspection utilities for Nautilus procedural macros.

#![warn(clippy::pedantic)]

use syn::{
    Field, Ident, LitStr, Token, Type,
    parse::{Parse, ParseStream},
};

/// A named field used by the retained Serde custom-data macro.
pub(crate) struct FieldSpec {
    /// The field identifier.
    pub ident: Ident,
    /// The field type.
    pub ty: Type,
}

/// A single macro option of the form `ident` or `ident = "value"`.
pub(crate) struct CustomDataOption {
    /// The option name.
    pub ident: Ident,
    /// The optional string value.
    pub value: Option<LitStr>,
}

impl Parse for CustomDataOption {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.parse()?;
        let value = if input.parse::<Option<Token![=]>>()?.is_some() {
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self { ident, value })
    }
}

/// Rejects retired catalog field options on a Serde custom-data field.
///
/// # Errors
///
/// Returns an error if an attribute fails to parse or contains an unknown option.
pub(crate) fn parse_field_options(field: &Field) -> Result<(), syn::Error> {
    if let Some(attr) = field.attrs.iter().find(|attr| {
        attr.path()
            .get_ident()
            .is_some_and(|ident| ident == "custom_data_field")
    }) {
        return Err(syn::Error::new_spanned(
            attr,
            "Catalog field options are retired; use Serde attributes",
        ));
    }
    Ok(())
}
