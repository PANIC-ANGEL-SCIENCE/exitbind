//! Per-observation child ownership. Linux reaping stays in an exclusive exec.
use super::capture::{process_group_exists, ObservationRequest};
use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
#[path = "capture_supervisor/linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::entry;

pub(super) struct ObservedChild {
    child: Child,
    #[cfg(target_os = "linux")]
    proof: linux::Proof,
}

impl std::ops::Deref for ObservedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl std::ops::DerefMut for ObservedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl ObservedChild {
    pub(super) fn spawn(
        request: &ObservationRequest,
        stdout: Stdio,
        stderr: Stdio,
    ) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::spawn(request, stdout, stderr)
        }
        #[cfg(not(target_os = "linux"))]
        {
            use std::os::unix::process::CommandExt;
            let child = Command::new("sh")
                .args(["-c", &request.command, "exitbind-observe-check"])
                .current_dir(&request.working_dir)
                .envs(
                    request
                        .preparation
                        .as_ref()
                        .map(|value| value.environment())
                        .unwrap_or_default(),
                )
                .stdout(stdout)
                .stderr(stderr)
                .process_group(0)
                .spawn()?;
            Ok(Self { child })
        }
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        #[cfg(target_os = "linux")]
        {
            self.proof.poll()?;
            let Some(status) = self.child.try_wait()? else {
                return Ok(None);
            };
            self.proof.finish(status)?;
            if self.proof.launch_failed() {
                return Err(io::Error::other(
                    "check command could not be launched by observer capsule",
                ));
            }
            Ok(self.proof.status())
        }
        #[cfg(not(target_os = "linux"))]
        self.child.try_wait()
    }

    pub(super) fn status(&mut self) -> Option<ExitStatus> {
        #[cfg(target_os = "linux")]
        {
            let _ = self.proof.poll();
            self.proof.status()
        }
        #[cfg(not(target_os = "linux"))]
        self.child.try_wait().ok().flatten()
    }

    pub(super) fn process_started(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            !self.proof.launch_failed()
        }
        #[cfg(not(target_os = "linux"))]
        true
    }

    pub(super) fn ended(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.proof.ended()
        }
        #[cfg(not(target_os = "linux"))]
        {
            i32::try_from(self.child.id())
                .ok()
                .and_then(|pid| process_group_exists(pid).ok())
                == Some(false)
        }
    }

    pub(super) fn cancelled(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.proof.cancelled()
        }
        #[cfg(not(target_os = "linux"))]
        false
    }

    pub(super) fn timed_out(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.proof.timed_out()
        }
        #[cfg(not(target_os = "linux"))]
        false
    }

    pub(super) fn terminate(&mut self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            // The capsule owns every checker signal. Never signal its group:
            // the reaper must survive hard termination of checker children.
            let pid = i32::try_from(self.child.id()).map_err(|e| e.to_string())?;
            if linux::signal_owned(pid, libc::SIGTERM).map_err(|e| e.to_string())? {
                let deadline = Instant::now() + Duration::from_millis(450);
                while Instant::now() < deadline {
                    match self.try_wait() {
                        Ok(Some(_)) => {
                            return if self.ended() {
                                Ok(())
                            } else {
                                Err("observer capsule could not prove owned cleanup".into())
                            };
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                linux::signal_owned(pid, libc::SIGKILL).map_err(|e| e.to_string())?;
            }
            // A missing final proof is always unknown, including forced death.
            let deadline = Instant::now() + Duration::from_millis(50);
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(Some(status)) => {
                        return self
                            .proof
                            .finish(status)
                            .and_then(|()| {
                                if self.ended() {
                                    Ok(())
                                } else {
                                    Err(io::Error::other("incomplete observer cleanup proof"))
                                }
                            })
                            .map_err(|e| e.to_string());
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(e) => return Err(e.to_string()),
                }
            }
            Err("observer capsule did not end before cleanup deadline".into())
        }
        #[cfg(not(target_os = "linux"))]
        super::capture::terminate_process_group(&mut self.child)
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
#[path = "capture_supervisor/tests.rs"]
mod tests;
