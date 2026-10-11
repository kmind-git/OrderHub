// SPDX-License-Identifier: LGPL-3.0-only

//! Versioned offline conversion of retained historical catalog data.

pub mod arrow;
pub mod catalog;
mod flat_depth;

#[cfg(test)]
mod catalog_tests;
