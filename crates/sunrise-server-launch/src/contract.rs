//! The dedicated game's launch contract (replaced by the full launch check in PR A).

use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command};

/// The dedicated launch contracts a server manager supports (inclusive).
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct ContractRange {
    pub min: u32,
    pub max: u32,
}

pub const SUPPORTED_CONTRACTS: ContractRange = ContractRange { min: 1, max: 2 };

#[derive(Deserialize)]
pub struct GameContract {
    pub sunrise: String,
    dedicated: Option<Dedicated>,
}

#[derive(Deserialize)]
struct Dedicated {
    contract: u32,
}

impl GameContract {
    pub fn contract(&self) -> u32 {
        self.dedicated.as_ref().map_or(1, |info| info.contract)
    }
}

/// Read the installed game's contract without checking it against this manager's range.
pub fn probe_contract(program: &Path) -> Result<GameContract, String> {
    let output = Command::new(program)
        .arg("--launch-api")
        .output()
        .map_err(|e| format!("{} --launch-api: {e}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} --launch-api: {}",
            program.display(),
            output.status
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("{} --launch-api: {e}", program.display()))
}
