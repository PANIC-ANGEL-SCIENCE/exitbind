//! Operation-local, rechecked physical reads for one ordinary carry chain.
//!
//! The carry verifier still executes for every requested ledger. This owner
//! only shares the securely read bytes while one Work projection is staged.
use super::{
    claim_path, claim_with_identity, ledger_path, load_at_unchecked, load_at_with_identity,
};
use super::{LedgerPath, Loaded, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct LedgerWitness {
    ledger: LedgerPath,
    events: Vec<Value>,
    source: String,
    identity: (u64, u64),
}

struct ClaimWitness {
    value: Option<Value>,
    source: Option<(String, (u64, u64))>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Active,
    Unsupported,
    Failed,
}

pub(crate) struct Observation {
    config: PathBuf,
    source: String,
    product_root: PathBuf,
    control_root: PathBuf,
    state_root: PathBuf,
    project_id: Option<String>,
    paths: BTreeMap<String, LedgerPath>,
    ledgers: BTreeMap<PathBuf, LedgerWitness>,
    claims: BTreeMap<PathBuf, ClaimWitness>,
    last_requested: Option<String>,
    sealed: bool,
    mode: Mode,
}

impl Observation {
    pub(crate) fn new(loaded: &Loaded) -> Self {
        Self {
            config: loaded.path.clone(),
            source: loaded.source.clone(),
            product_root: loaded.product_root.clone(),
            control_root: loaded.control_root.clone(),
            state_root: loaded.state_root.clone(),
            project_id: loaded.project_id.clone(),
            paths: BTreeMap::new(),
            ledgers: BTreeMap::new(),
            claims: BTreeMap::new(),
            last_requested: None,
            sealed: false,
            // The non-Unix file identity helper does not distinguish replacement.
            mode: if cfg!(unix) {
                Mode::Active
            } else {
                Mode::Unsupported
            },
        }
    }

    pub(crate) fn same_scope(&self, loaded: &Loaded) -> bool {
        self.config == loaded.path
            && self.source == loaded.source
            && self.product_root == loaded.product_root
            && self.control_root == loaded.control_root
            && self.state_root == loaded.state_root
            && self.project_id == loaded.project_id
    }

    pub(crate) fn seal_for(&mut self, requested: &str) {
        if !self.sealed {
            self.sealed = true;
            if !self.paths.contains_key(requested)
                || self.last_requested.as_deref() != Some(requested)
            {
                self.disable();
            }
        }
    }

    pub(crate) fn fallback(&self) -> bool {
        self.mode == Mode::Unsupported
    }

    pub(crate) fn failed(&self) -> bool {
        self.mode == Mode::Failed
    }

    pub(crate) fn stop_on_error(&mut self) {
        self.mode = Mode::Failed;
    }

    pub(crate) fn disable(&mut self) {
        if self.mode == Mode::Active {
            self.mode = Mode::Unsupported;
        }
    }

    pub(crate) fn path(
        &mut self,
        loaded: &Loaded,
        requested: &str,
        allow_missing: bool,
    ) -> Result<LedgerPath, String> {
        if !self.same_scope(loaded) {
            self.disable();
        }
        if self.mode == Mode::Active {
            if let Some(path) = self.paths.get(requested) {
                return Ok(path.clone());
            }
            if self.sealed {
                self.disable();
            }
        }
        let path = ledger_path(&loaded.state_root, requested, allow_missing)?;
        if self.mode == Mode::Active {
            self.paths.insert(requested.to_owned(), path.clone());
        }
        Ok(path)
    }

    pub(crate) fn read(
        &mut self,
        loaded: &Loaded,
        ledger: &LedgerPath,
    ) -> Result<(LedgerPath, Vec<Value>, String), String> {
        if !self.same_scope(loaded) {
            self.disable();
        }
        if self.mode == Mode::Active {
            if let Some(witness) = self.ledgers.get(&ledger.expected) {
                return Ok((
                    witness.ledger.clone(),
                    witness.events.clone(),
                    witness.source.clone(),
                ));
            }
            if self.sealed {
                self.disable();
            }
        }
        if self.mode != Mode::Active {
            return load_at_unchecked(loaded, ledger);
        }
        let (ledger, events, source, identity) = load_at_with_identity(loaded, ledger)?;
        if let Some(start) = events.first() {
            let linked = start.get("supersedes").is_some();
            if (linked && start["carryProtocol"].as_u64() != Some(crate::run::carry::PROTOCOL))
                || start.get("checkReplacement").is_some()
                || start.get("preReturnPolicy").is_some()
                || start["configSha256"] != crate::evidence::hash::text(&loaded.source)
                || events.iter().any(|event| {
                    matches!(
                        event["action"].as_str(),
                        Some("check_observation_recovered" | "pre_return_reconciled")
                    )
                })
            {
                self.disable();
            }
        }
        self.ledgers.insert(
            ledger.expected.clone(),
            LedgerWitness {
                ledger: ledger.clone(),
                events: events.clone(),
                source: source.clone(),
                identity,
            },
        );
        Ok((ledger, events, source))
    }

    pub(crate) fn claim(&mut self, path: &Path) -> Result<Option<Value>, String> {
        if self.mode == Mode::Active {
            if let Some(witness) = self.claims.get(path) {
                return Ok(witness.value.clone());
            }
            if self.sealed
                && !self
                    .ledgers
                    .values()
                    .any(|node| claim_path(&node.ledger) == path)
            {
                self.disable();
            }
        }
        if self.mode != Mode::Active {
            return super::valid_claim(path);
        }
        let (value, source) = claim_with_identity(path)?;
        if value.as_ref().is_some_and(|claim| {
            claim["carryProtocol"].as_u64() != Some(crate::run::carry::PROTOCOL)
                || claim.get("checkReplacement").is_some()
                || claim.get("preReturnPolicy").is_some()
        }) {
            self.disable();
        }
        self.claims.insert(
            path.to_path_buf(),
            ClaimWitness {
                value: value.clone(),
                source,
            },
        );
        Ok(value)
    }

    pub(crate) fn load(
        &mut self,
        loaded: &Loaded,
        requested: &str,
    ) -> Result<(LedgerPath, Vec<Value>, String), String> {
        self.last_requested = Some(requested.to_owned());
        let ledger = self.path(loaded, requested, false)?;
        let result = self.read(loaded, &ledger)?;
        crate::run::carry::validate_chain_observed(loaded, &ledger, &result.1, self)?;
        Ok(result)
    }

    /// Reopen the exact paths and compare full bytes and file identities. An
    /// absent successor claim is observed explicitly and must remain absent.
    pub(crate) fn finish(self, loaded: &Loaded) -> Result<bool, String> {
        if !self.same_scope(loaded) {
            return Err("carry observation configuration changed during projection".into());
        }
        for (requested, path) in &self.paths {
            let fresh = ledger_path(&loaded.state_root, requested, true)
                .map_err(|_| "carry observation path changed during projection")?;
            if fresh.root != path.root || fresh.expected != path.expected {
                return Err("carry observation path changed during projection".into());
            }
        }
        for witness in self.ledgers.values() {
            let (_, _, source, identity) = load_at_with_identity(loaded, &witness.ledger)
                .map_err(|_| "carry observation ledger changed during projection")?;
            if source != witness.source || identity != witness.identity {
                return Err("carry observation ledger changed during projection".into());
            }
        }
        for (path, witness) in &self.claims {
            let (value, source) = claim_with_identity(path)
                .map_err(|_| "carry observation claim changed during projection")?;
            if source != witness.source || value != witness.value {
                return Err("carry observation claim changed during projection".into());
            }
        }
        Ok(self.mode == Mode::Unsupported)
    }
}
