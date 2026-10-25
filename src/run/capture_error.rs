//! Bounded typed observation diagnostics; these are never check results.
//!
//! A failure that kept an owned checker group records it as the optional
//! `ownedProcessGroup` fact, so a later read can observe that group by the
//! identity recorded here instead of trusting a stale absence claim.
use super::*;
use crate::run::capture_supervisor::OwnedGroup;
use crate::run::state::check_observation::{OWNED_GROUP_FIELD, OWNED_START_FIELD};

#[derive(Clone, Debug)]
pub(crate) enum CaptureDisposition {
    Disabled,
    Complete {
        stdout_bytes: u64,
        stderr_bytes: u64,
    },
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) struct ExecutionOutcome {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) duration_ms: u64,
    pub(crate) capture: CaptureDisposition,
    pub(crate) group_ended: bool,
    pub(crate) process_started: bool,
}

pub(crate) struct ObservationError {
    pub(super) message: String,
    pub(super) outcome: ExecutionOutcome,
    pub(super) cleanup_errors: Vec<String>,
    pub(super) remaining_paths: Vec<PathBuf>,
    pub(super) deadline_exceeded: bool,
    pub(super) capture_readers_ended: bool,
    pub(super) partial_captures: Vec<Value>,
    pub(super) owned_group: Option<OwnedGroup>,
}

impl ObservationError {
    pub(super) fn new(message: impl Into<String>, outcome: ExecutionOutcome) -> Self {
        Self {
            message: message.into(),
            outcome,
            cleanup_errors: Vec::new(),
            remaining_paths: Vec::new(),
            deadline_exceeded: false,
            capture_readers_ended: true,
            partial_captures: Vec::new(),
            owned_group: None,
        }
    }

    /// Record the owned checker process group the capsule observed, with its
    /// leader start marker when it could be read. None means no identity is
    /// known, never that no process exists.
    pub(super) fn with_owned_group(mut self, owned: Option<OwnedGroup>) -> Self {
        self.owned_group = owned;
        self
    }

    pub(super) fn with_cleanup(
        mut self,
        owned: &OwnedPaths,
        cleanup_errors: impl IntoIterator<Item = String>,
    ) -> Self {
        self.cleanup_errors.extend(cleanup_errors);
        self.remaining_paths = owned.remaining();
        self
    }
}

impl fmt::Display for ObservationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = self
            .outcome
            .status
            .as_ref()
            .map(|status| format!("{:?}", status))
            .unwrap_or_else(|| "unavailable".into());
        let capture = match &self.outcome.capture {
            CaptureDisposition::Disabled => "disabled".to_owned(),
            CaptureDisposition::Failed => "failed".to_owned(),
            CaptureDisposition::Complete {
                stdout_bytes,
                stderr_bytes,
            } => format!("complete(stdout={stdout_bytes}, stderr={stderr_bytes})"),
        };
        write!(
            output,
            "{} (status={status}, durationMs={}, capture={capture})",
            self.message, self.outcome.duration_ms
        )?;
        if !self.cleanup_errors.is_empty() {
            write!(
                output,
                "; cleanup failed: {}",
                self.cleanup_errors.join("; ")
            )?;
        }
        if !self.remaining_paths.is_empty() {
            let paths = self
                .remaining_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            write!(output, "; owned paths remain: {}", paths.join(", "))?;
        }
        Ok(())
    }
}

impl fmt::Debug for ObservationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, output)
    }
}

impl ObservationError {
    pub(crate) fn facts(&self) -> Value {
        let result = self
            .outcome
            .status
            .as_ref()
            .and_then(|status| observed_result(status).ok());
        let mut facts = json!({
            "process": result,
            "processStarted": self.outcome.process_started,
            "groupEnded": self.outcome.group_ended,
            "captureReadersEnded": self.capture_readers_ended,
            "captureAvailability": if self.partial_captures.is_empty() { "unavailable" } else { "partial" },
            "durationMs": self.outcome.duration_ms,
            "capture": match self.outcome.capture { CaptureDisposition::Disabled => "disabled", CaptureDisposition::Complete { .. } => "complete", CaptureDisposition::Failed => "incomplete" },
            "deadlineExceeded": self.deadline_exceeded,
            "partialCaptures": self.partial_captures,
            "storageStage": "capture",
            "storage": "not_committed",
            "termination": if self.outcome.group_ended && self.capture_readers_ended && self.cleanup_errors.is_empty() { "ended" } else { "unknown" },
            "diagnostic": self.message.chars().filter(|c| !c.is_control()).take(1024).collect::<String>(),
            "cleanupErrorCount": self.cleanup_errors.len(),
            "remainingOwnedPathCount": self.remaining_paths.len(),
        });
        // Written only when known: a failure with no observed group keeps the
        // exact fifteen-fact shape every earlier reader already validates.
        if let Some(owned) = &self.owned_group {
            facts[OWNED_GROUP_FIELD] = json!(owned.group);
            if let Some(ticks) = owned.start_time_ticks {
                facts[OWNED_START_FIELD] = json!(ticks);
            }
        }
        facts
    }
}

pub(super) fn retain_partial(request: &ObservationRequest, owned: &OwnedPaths) -> Vec<Value> {
    let mut refs = Vec::new();
    for (index, path) in owned.0.iter().enumerate() {
        let saved = (|| {
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let file = options.open(path).ok()?;
            if !file.metadata().ok()?.is_file() {
                return None;
            }
            let mut bytes = Vec::new();
            file.take(request.capture.per_stream_bytes + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 > request.capture.per_stream_bytes {
                return None;
            }
            let name = format!(
                "check-partial-{}-{}-{index}.raw",
                request.operation_id,
                timestamp_nanos()
            );
            let destination = request.artifact_dir.join(name);
            let mut output = crate::project::managed_files::open_state_file(&destination).ok()?;
            output.write_all(&bytes).ok()?;
            output.sync_all().ok()?;
            let relative = destination
                .strip_prefix(&request.state_root)
                .ok()?
                .to_str()?;
            Some(
                json!({"root": "state", "path": relative, "sha256": crate::evidence::hash::bytes(&bytes),
                "bytes": bytes.len(), "stream": if index == 0 { "stdout" } else { "stderr" },
                "completeness": "partial", "perStreamLimit": request.capture.per_stream_bytes}),
            )
        })();
        if let Some(saved) = saved {
            refs.push(saved);
        }
    }
    refs
}
