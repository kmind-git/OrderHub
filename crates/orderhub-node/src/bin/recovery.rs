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

//! Offline recovery operations; the live node must release its exclusive database lock first.

use orderhub_gateway::journal::{BusinessJournal, RecoveryIdentity};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, source] if command == "inspect" || command == "upgrade" => {
            require_source(Path::new(source))?;
            let journal = BusinessJournal::open(Path::new(source))?;
            println!("{}", serde_json::to_string_pretty(&journal.verify_recovery()?)?);
        }
        [command, source, directory] if command == "backup" => {
            require_source(Path::new(source))?;
            let journal = BusinessJournal::open(Path::new(source))?;
            let manifest = journal.checkpoint(Path::new(directory), RecoveryIdentity {
                instance_id: journal.instance_id()?,
                binary_hash: blake3::hash(&std::fs::read(std::env::current_exe()?)?).to_hex().to_string(),
                configuration_hash: blake3::hash(b"offline-recovery-operator-v2").to_hex().to_string(),
            })?;
            println!("{}", manifest.display());
        }
        [command, manifest, target] if command == "restore" => {
            let journal = BusinessJournal::restore_backup(Path::new(manifest), Path::new(target))?;
            println!("{}", serde_json::to_string_pretty(&journal.verify_recovery()?)?);
        }
        _ => return Err("usage: orderhub-recovery inspect|upgrade JOURNAL | backup JOURNAL DIRECTORY | restore MANIFEST NEW-JOURNAL".into()),
    }
    Ok(())
}

fn require_source(path: &Path) -> Result<(), std::io::Error> {
    if path.is_file() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "recovery source must be an existing journal file",
        ))
    }
}
