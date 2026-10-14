use super::*;

impl Invoker {
    pub(super) fn new() -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("benchmark cannot resolve its executable: {error}"))?;
        Ok(Self {
            executable,
            total: 0,
            successful: 0,
            failed: 0,
            actions: BTreeMap::new(),
        })
    }

    pub(super) fn call<I, S>(&mut self, cwd: &Path, arguments: I) -> Result<ChildOutput, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let arguments = arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_os_string())
            .collect::<Vec<_>>();
        let action = action_label(&arguments)?;
        let output = Command::new(&self.executable)
            .args(&arguments)
            .current_dir(cwd)
            .output()
            .map_err(|error| format!("benchmark could not invoke its binary: {error}"))?;
        self.total += 1;
        if output.status.success() {
            self.successful += 1;
        } else {
            self.failed += 1;
        }
        *self.actions.entry(action).or_insert(0) += 1;
        Ok(ChildOutput {
            status: output.status,
            stdout: output.stdout,
        })
    }

    pub(super) fn counts(&self) -> Value {
        json!({
            "total": self.total,
            "successful": self.successful,
            "failed": self.failed,
            "byAction": self.actions,
        })
    }
}

impl ChildOutput {
    pub(super) fn exit_code(&self, label: &str) -> Result<i32, String> {
        self.status
            .code()
            .ok_or_else(|| format!("{label} terminated without an exit code"))
    }

    pub(super) fn json(&self, label: &str) -> Result<Value, String> {
        serde_json::from_slice(&self.stdout)
            .map_err(|error| format!("{label} returned invalid JSON: {error}"))
    }
}

impl TempFixture {
    pub(super) fn create() -> Result<Self, String> {
        let base = std::env::temp_dir();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("benchmark clock is before the Unix epoch: {error}"))?
            .as_nanos();
        for attempt in 0..32u32 {
            let candidate = base.join(format!(
                "{}-value-proof-{}-{}-{}",
                if exitbind_surface() {
                    "exitbind"
                } else {
                    "soulmate"
                },
                std::process::id(),
                seed,
                attempt
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    if let Err(error) = set_dir_mode(&candidate, 0o700) {
                        let _ = fs::remove_dir(&candidate);
                        return Err(format!(
                            "benchmark fixture permissions cannot be set: {error}"
                        ));
                    }
                    let path = fs::canonicalize(&candidate).map_err(|error| {
                        let _ = fs::remove_dir(&candidate);
                        format!("benchmark fixture cannot be resolved: {error}")
                    })?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("benchmark fixture cannot be created: {}", error))
                }
            }
        }
        Err("benchmark fixture name could not be made unique".into())
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
            .unwrap_or(true)
        {
            return;
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn args<const N: usize>(values: [&str; N]) -> Vec<OsString> {
    values.into_iter().map(OsString::from).collect()
}

pub(super) fn args_with<I, S>(values: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    values
        .into_iter()
        .map(|value| value.as_ref().to_os_string())
        .collect()
}

pub(super) fn action_label(arguments: &[OsString]) -> Result<String, String> {
    match (arguments.first(), arguments.get(1)) {
        (Some(command), Some(action)) if command == "run" => Ok(format!(
            "run {}",
            action.to_str().ok_or("benchmark action is not UTF-8")?
        )),
        (Some(command), _) => Ok(command
            .to_str()
            .ok_or("benchmark command is not UTF-8")?
            .to_owned()),
        _ => Err("benchmark command is missing".into()),
    }
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
