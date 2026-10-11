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

//! Standard startup, system, and version headers for log output.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, Ordering};

use nautilus_core::{UUID4, consts::NAUTILUS_VERSION_CORE};
use nautilus_model::{
    identifiers::TraderId,
    types::fixed::{FIXED_PRECISION, HIGH_PRECISION_MODE, PRECISION_BYTES},
};
use sysinfo::System;
use ustr::Ustr;

use crate::{enums::LogColor, logging::log_info};

const GIT_COMMIT_LEN: usize = 12;
static MIMALLOC_REGISTERED: AtomicBool = AtomicBool::new(false);

#[cfg(panic = "abort")]
const PANIC_STRATEGY: &str = "abort";
#[cfg(panic = "unwind")]
const PANIC_STRATEGY: &str = "unwind";

const BUILD_VERSIONS: &[(&str, &str)] = &[
    ("git_commit", env!("NAUTILUS_BUILD_GIT_COMMIT")),
    ("rustc", env!("NAUTILUS_BUILD_RUSTC_VERSION")),
    ("target", env!("NAUTILUS_BUILD_TARGET")),
    ("profile", env!("NAUTILUS_BUILD_PROFILE")),
    ("panic", PANIC_STRATEGY),
    ("cargo_lock", env!("NAUTILUS_BUILD_CARGO_LOCK_CRC32")),
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    ("libc_crate", env!("NAUTILUS_BUILD_LIBC_VERSION")),
    ("rust_decimal", env!("NAUTILUS_BUILD_RUST_DECIMAL_VERSION")),
];

#[cfg(feature = "live")]
const LIVE_VERSIONS: &[(&str, &str)] = &[
    ("rustls", env!("NAUTILUS_BUILD_RUSTLS_VERSION")),
    ("aws_lc_rs", env!("NAUTILUS_BUILD_AWS_LC_RS_VERSION")),
    ("tokio", env!("NAUTILUS_BUILD_TOKIO_VERSION")),
];

/// Records that the final artifact uses mimalloc as its Rust global allocator.
///
/// This only affects version header metadata. The final artifact must declare mimalloc as its
/// global allocator before calling this function.
pub fn register_allocator_mimalloc() {
    MIMALLOC_REGISTERED.store(true, Ordering::Release);
}

/// Logs the Nautilus startup header with system, identifier, and version details.
#[rustfmt::skip]
pub fn log_header(trader_id: TraderId, machine_id: &str, instance_id: UUID4, component: Ustr) {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let c = component;

    let kernel_version = System::kernel_version().map_or(String::new(), |v| format!("kernel-{v} "));
    let os_version = System::long_os_version().unwrap_or_default();
    let pid = std::process::id();

    header_sepr(c, "=================================================================");
    header_sepr(c, " NAUTILUS TRADER - Automated Algorithmic Trading Platform");
    header_sepr(c, " by Nautech Systems Pty Ltd.");
    header_sepr(c, " Copyright (C) 2015-2026. All rights reserved.");
    header_sepr(c, "=================================================================");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣀⣀⡤⠤⠤⠤⠤⠤⠤⠤⢤⡀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⠤⠖⠚⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣸⠁⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⢀⣠⠖⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⡴⠚⠁⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⢀⡴⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡞⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⣠⠏⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⠤⠖⠒⠒⠒⠒⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⡇⠀⠀⠀⠀⠀⠀⠀⢀⡖⠋⣠⢴⢪⠞⣩⢟⡭⠵⢤⣤⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⣦⡙⠦⣄⡀⠀⢀⣠⠴⠋⢠⡐⣇⢸⡘⢦⡇⣏⡴⣋⡭⠖⠮⢥⡀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⢸⡉⠓⠦⠭⠭⠭⠴⣺⠃⠸⣷⢬⣓⣛⠒⠩⣌⢡⡷⢒⣫⠭⣝⡛⠆⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠙⠦⢤⣀⣠⠤⠞⣡⠞⡆⠺⣭⣭⡷⢇⣷⡻⠡⠾⣛⣒⠦⢤⡙⡆⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠈⠳⣖⠒⠒⠒⠋⠁⣠⠇⡼⢦⠀⣌⡉⢥⣄⣛⡻⢥⡈⠉⢳⡙⠇⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠈⠙⣒⣒⣒⣋⡥⠞⠁⣸⠃⡇⠙⢦⢹⠀⠙⡆⢳⢀⡴⠃⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠈⠙⠒⠦⠤⠴⣚⣡⠞⠁⣠⠏⡼⢀⣠⠇⠞⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠉⠉⠉⠑⠚⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_sepr(c, "=================================================================");
    header_sepr(c, " SYSTEM SPECIFICATION");
    header_sepr(c, "=================================================================");

    if let Some(cpu) = sys.cpus().first() {
        header_line(c, &format!("CPU architecture: {}", cpu.brand()));
        header_line(c, &format!("CPU(s): {} @ {} MHz", sys.cpus().len(), cpu.frequency()));
    } else {
        header_line(c, "CPU: unknown");
    }
    header_line(c, &format!("OS: {kernel_version}{os_version}"));

    log_sysinfo(component);

    header_sepr(c, "=================================================================");
    header_sepr(c, " IDENTIFIERS");
    header_sepr(c, "=================================================================");
    header_line(c, &format!("trader_id: {trader_id}"));
    header_line(c, &format!("machine_id: {machine_id}"));
    header_line(c, &format!("instance_id: {instance_id}"));
    header_line(c, &format!("PID: {pid}"));

    header_sepr(c, "=================================================================");
    header_sepr(c, " VERSIONING");
    header_sepr(c, "=================================================================");


    log_rust_versioning(c);



    header_sepr(c, "=================================================================");
}

#[rustfmt::skip]
fn log_rust_versioning(c: Ustr) {
    header_line(c, &format!("nautilus_trader: {NAUTILUS_VERSION_CORE}"));
    log_build_versioning(c);
    log_optional_versioning(c);
}

#[rustfmt::skip]
fn log_build_versioning(c: Ustr) {
    for (name, version) in build_versions() {
        header_line(c, &format!("{name}: {version}"));
    }
}

#[rustfmt::skip]
fn log_optional_versioning(c: Ustr) {

    #[cfg(feature = "live")]
    for (name, version) in live_versions() {
        header_line(c, &format!("{name}: {version}"));
    }



    #[cfg(not(feature = "live"))]
    let _ = c;
}

#[cfg(feature = "live")]
fn live_versions() -> impl Iterator<Item = (&'static str, &'static str)> {
    LIVE_VERSIONS
        .iter()
        .copied()
        .filter(|(_, version)| !version.is_empty())
}

fn build_versions() -> Vec<(&'static str, String)> {
    let mut versions = BUILD_VERSIONS
        .iter()
        .filter(|(_, version)| !version.is_empty())
        .map(|(name, version)| (*name, display_version(name, version)))
        .collect::<Vec<_>>();

    let precision_index = versions
        .iter()
        .position(|(name, _)| *name == "panic")
        .map_or(versions.len(), |index| index + 1);
    versions.insert(precision_index, ("precision", precision_version()));

    let allocator_index = versions
        .iter()
        .position(|(name, _)| *name == "precision")
        .map_or(versions.len(), |index| index + 1);
    versions.insert(allocator_index, ("allocator", allocator_version()));

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        if let Some(version) = libc_runtime_version() {
            let index = versions
                .iter()
                .position(|(name, _)| matches!(*name, "cargo_lock" | "libc_crate"))
                .unwrap_or(versions.len());
            versions.insert(index, ("libc_runtime", version));
        }
    }

    versions
}

fn precision_version() -> String {
    let mode = if HIGH_PRECISION_MODE == 1 {
        "high"
    } else {
        "standard"
    };
    let bits = PRECISION_BYTES * 8;
    format!("{mode} ({bits}-bit, {FIXED_PRECISION} dp)")
}

fn allocator_version() -> String {
    allocator_version_for(MIMALLOC_REGISTERED.load(Ordering::Acquire))
}

fn allocator_version_for(mimalloc: bool) -> String {
    if mimalloc {
        let version = env!("NAUTILUS_BUILD_MIMALLOC_VERSION");
        if version.is_empty() {
            "mimalloc".to_string()
        } else {
            format!("mimalloc {version}")
        }
    } else {
        "system".to_string()
    }
}

fn display_version(name: &str, version: &str) -> String {
    match name {
        "cargo_lock" => version
            .strip_prefix("crc32:")
            .unwrap_or(version)
            .to_string(),
        "git_commit" => version.get(..GIT_COMMIT_LEN).unwrap_or(version).to_string(),
        _ => version.to_string(),
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
fn libc_runtime_version() -> Option<String> {
    // SAFETY: glibc owns the returned static null-terminated string.
    let version = unsafe { libc::gnu_get_libc_version() };
    if version.is_null() {
        return None;
    }

    // SAFETY: glibc documents the returned pointer as a valid C string.
    let version = unsafe { CStr::from_ptr(version) }.to_str().ok()?;
    Some(format!("glibc {version}"))
}

/// Logs current memory and swap usage.
#[rustfmt::skip]
pub fn log_sysinfo(component: Ustr) {
    let mut sys = System::new();
    sys.refresh_memory();

    let c = component;

    let ram_total = sys.total_memory();
    let ram_used = sys.used_memory();
    let ram_used_p = (ram_used as f64 / ram_total as f64) * 100.0;
    let ram_avail = ram_total - ram_used;
    let ram_avail_p = (ram_avail as f64 / ram_total as f64) * 100.0;

    header_sepr(c, "=================================================================");
    header_sepr(c, " MEMORY USAGE");
    header_sepr(c, "=================================================================");
    header_line(c, &format!("RAM-Total: {:.2} GiB", bytes_to_gib(ram_total)));
    header_line(c, &format!("RAM-Used: {:.2} GiB ({:.2}%)", bytes_to_gib(ram_used), ram_used_p));
    header_line(c, &format!("RAM-Avail: {:.2} GiB ({:.2}%)", bytes_to_gib(ram_avail), ram_avail_p));

    let swap_total = sys.total_swap();
    if swap_total > 0 {
        let swap_used = sys.used_swap();
        let swap_used_p = (swap_used as f64 / swap_total as f64) * 100.0;
        let swap_avail = swap_total.saturating_sub(swap_used);
        let swap_avail_p = (swap_avail as f64 / swap_total as f64) * 100.0;
        header_line(c, &format!("Swap-Total: {:.2} GiB", bytes_to_gib(swap_total)));
        header_line(c, &format!("Swap-Used: {:.2} GiB ({:.2}%)", bytes_to_gib(swap_used), swap_used_p));
        header_line(c, &format!("Swap-Avail: {:.2} GiB ({:.2}%)", bytes_to_gib(swap_avail), swap_avail_p));
    } else {
        header_line(c, "Swap: disabled");
    }
}

fn header_sepr(c: Ustr, s: &str) {
    log_info!("{}", s, color = LogColor::Cyan, component = c.as_str());
}

fn header_line(c: Ustr, s: &str) {
    log_info!("{}", s, component = c.as_str());
}

fn bytes_to_gib(b: u64) -> f64 {
    b as f64 / (2u64.pow(30) as f64)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_build_versions_match_compiled_metadata() {
        let versions = build_versions();

        for (name, expected) in BUILD_VERSIONS {
            let expected = (!expected.is_empty()).then(|| display_version(name, expected));
            assert_eq!(version(&versions, name), expected.as_deref());
        }
    }

    #[rstest]
    fn test_build_modes_match_compiled_configuration() {
        let versions = build_versions();
        let panic_index = versions
            .iter()
            .position(|(name, _)| *name == "panic")
            .expect("panic version should be present");
        let precision_index = versions
            .iter()
            .position(|(name, _)| *name == "precision")
            .expect("precision version should be present");
        let allocator_index = versions
            .iter()
            .position(|(name, _)| *name == "allocator")
            .expect("allocator version should be present");

        assert_eq!(version(&versions, "panic"), Some(PANIC_STRATEGY));
        assert_eq!(precision_index, panic_index + 1);
        assert_eq!(allocator_index, precision_index + 1);
        assert_eq!(
            version(&versions, "precision"),
            Some(precision_version().as_str()),
        );
        assert_eq!(version(&versions, "allocator"), Some("system"),);
    }

    /// Pins the header against the precision constants rather than restating the format
    /// expression, so a change to either the mode check or the byte-to-bit conversion fails.
    #[rstest]
    fn test_precision_version_reports_the_compiled_precision_mode() {
        let (expected_mode, expected_bits) = match PRECISION_BYTES {
            16 => ("high", 128),
            8 => ("standard", 64),
            other => panic!("unexpected PRECISION_BYTES {other}"),
        };

        assert_eq!(
            precision_version(),
            format!("{expected_mode} ({expected_bits}-bit, {FIXED_PRECISION} dp)")
        );
    }

    #[rstest]
    #[case(0, 0.0)]
    #[case(1 << 30, 1.0)]
    #[case(3 << 29, 1.5)]
    #[case((1 << 30) * 64, 64.0)]
    fn test_bytes_to_gib_converts_from_binary_gigabytes(#[case] bytes: u64, #[case] expected: f64) {
        assert_eq!(bytes_to_gib(bytes), expected);
    }

    #[rstest]
    #[case("cargo_lock", "crc32:0a1b2c3d", "0a1b2c3d")]
    #[case("cargo_lock", "0a1b2c3d", "0a1b2c3d")]
    #[case("git_commit", "0123456789abcdef0123", "0123456789ab")]
    #[case("git_commit", "0123abc", "0123abc")]
    #[case("rustc", "1.90.0", "1.90.0")]
    fn test_display_version_trims_only_the_named_fields(
        #[case] name: &str,
        #[case] version: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(display_version(name, version), expected);
    }

    #[rstest]
    fn test_allocator_version_formats() {
        let mimalloc = env!("NAUTILUS_BUILD_MIMALLOC_VERSION");
        let expected = if mimalloc.is_empty() {
            "mimalloc".to_string()
        } else {
            format!("mimalloc {mimalloc}")
        };

        assert_eq!(allocator_version_for(false), "system");
        assert_eq!(allocator_version_for(true), expected);
    }

    #[cfg(feature = "live")]
    #[rstest]
    fn test_live_versions_match_compiled_metadata() {
        let versions = live_versions().collect::<Vec<_>>();

        for (_, expected) in LIVE_VERSIONS {
            assert!(!expected.is_empty());
        }
        assert_eq!(versions, LIVE_VERSIONS);
        assert_eq!(versions.last().map(|(name, _)| *name), Some("tokio"));
    }

    #[rstest]
    fn test_cargo_lock_fingerprint_format() {
        const CRC32_HEX_LEN: usize = 8;

        let versions = build_versions();
        let Some(fingerprint) = version(&versions, "cargo_lock") else {
            return;
        };
        let embedded = env!("NAUTILUS_BUILD_CARGO_LOCK_CRC32");
        let Some(checksum) = embedded.strip_prefix("crc32:") else {
            panic!("embedded cargo_lock fingerprint should use the crc32 prefix");
        };

        assert_eq!(checksum.len(), CRC32_HEX_LEN);
        assert!(checksum.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(fingerprint.len(), CRC32_HEX_LEN);
        assert_eq!(fingerprint, checksum);
    }

    #[rstest]
    fn test_git_commit_format() {
        let versions = build_versions();
        let Some(commit) = version(&versions, "git_commit") else {
            return;
        };

        assert_eq!(commit.len(), GIT_COMMIT_LEN);
        assert!(commit.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[rstest]
    fn test_libc_runtime_version_reports_glibc() {
        let version = libc_runtime_version().expect("glibc should report its runtime version");
        let versions = build_versions();
        let runtime_index = versions
            .iter()
            .position(|(name, _)| *name == "libc_runtime")
            .expect("libc runtime version should be present");
        let cargo_lock_index = versions.iter().position(|(name, _)| *name == "cargo_lock");
        let libc_crate_index = versions.iter().position(|(name, _)| *name == "libc_crate");

        assert!(version.starts_with("glibc "));
        assert!(
            version["glibc ".len()..]
                .bytes()
                .any(|byte| byte.is_ascii_digit())
        );

        if let Some(cargo_lock_index) = cargo_lock_index {
            assert_eq!(cargo_lock_index, runtime_index + 1);
        }

        if let Some(libc_crate_index) = libc_crate_index {
            assert_eq!(
                libc_crate_index,
                cargo_lock_index.unwrap_or(runtime_index) + 1,
            );
        }
    }

    fn version<'a>(versions: &'a [(&str, String)], name: &str) -> Option<&'a str> {
        versions
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, version)| version.as_str())
    }
}
