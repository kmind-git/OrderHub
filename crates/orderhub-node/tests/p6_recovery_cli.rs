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

//! Real offline operator argument, backup, and no-clobber checks without external services.

use orderhub_gateway::journal::BusinessJournal;
use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orderhub-recovery"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn test_operator_rejects_bad_arguments_and_missing_sources_then_restores_fenced_backup() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../audit/orderhub-simplification-2026-10-02/run-16/runtime-tmp");
    std::fs::create_dir_all(&root).unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    let source = dir.path().join("source.redb");
    assert!(!run(&[]).status.success());
    assert!(
        !run(&["inspect".as_ref(), source.as_os_str()])
            .status
            .success()
    );
    assert!(!source.exists());
    let journal = BusinessJournal::open(&source).unwrap();
    let epoch = journal.epoch();
    drop(journal);
    assert!(
        run(&["inspect".as_ref(), source.as_os_str()])
            .status
            .success()
    );
    let backup = dir.path().join("backup");
    let output = run(&["backup".as_ref(), source.as_os_str(), backup.as_os_str()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    let target = dir.path().join("new.redb");
    let output = run(&["restore".as_ref(), manifest.as_os_str(), target.as_os_str()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let journal = BusinessJournal::open(&target).unwrap();
    assert_ne!(journal.epoch(), epoch);
    assert!(journal.verify_recovery().unwrap().backup_restore_fenced);
    assert!(
        !run(&["restore".as_ref(), manifest.as_os_str(), target.as_os_str()])
            .status
            .success()
    );
}
