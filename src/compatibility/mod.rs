//! The small, typed boundary for the current and historical command surfaces.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Surface {
    Exitbind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Profile {
    pub(crate) surface: Surface,
    pub(crate) caller: &'static str,
    pub(crate) product: &'static str,
    pub(crate) producer: &'static str,
    pub(crate) format_version: u64,
    pub(crate) config: &'static str,
    pub(crate) control: &'static str,
    pub(crate) state: &'static str,
    pub(crate) api: &'static str,
    pub(crate) raw_installer: &'static str,
    pub(crate) install_prefix_env: &'static str,
    pub(crate) cache: &'static str,
    pub(crate) marker: &'static str,
    pub(crate) installed_commands: &'static [&'static str],
}

/// Hook handshake tokens a current binary accepts from the CLI on PATH.
///
/// `crate::host::hooks::PROTOCOL` is still the historical spelling because every
/// published release compares it exactly; accepting the Exitbind spelling now
/// is what makes a later switch of the emitted token safe. Retirement criterion:
/// once no supported release older than the first one carrying this list can be
/// the binary that manages hooks, `host::hooks::PROTOCOL` may become
/// `exitbind-hook-v1` and the legacy token moves to read-only acceptance.
pub(crate) const ACCEPTED_HOOK_PROTOCOLS: [&str; 2] = ["soulmate-hook-v1", "exitbind-hook-v1"];

const EXITBIND_COMMANDS: &[&str] = &["exitbind"];
const EXITBIND: Profile = Profile {
    surface: Surface::Exitbind,
    caller: "exitbind",
    product: "exitbind",
    producer: "exitbind",
    format_version: 8,
    config: "exitbind.json",
    control: "exitbind",
    state: ".exitbind",
    api: "https://api.github.com/repos/envy404/exitbind/releases?per_page=20",
    raw_installer: "https://raw.githubusercontent.com/envy404/exitbind/",
    install_prefix_env: "EXITBIND_INSTALL_PREFIX",
    cache: "exitbind/update.json",
    marker: "exitbind/update.lock",
    installed_commands: EXITBIND_COMMANDS,
};

/// The executable profile is always Exitbind. Historical record identifiers
/// remain accepted by their owning readers below this boundary.
pub(crate) fn profile() -> Profile {
    EXITBIND
}

/// Retired copied binaries and symlink aliases must not silently acquire the
/// Exitbind configuration or write authority. Check the invocation spelling
/// before CLI parsing or configuration loading.
pub(crate) fn invoked_as_retired_soulmate(
    argv0: Option<&std::ffi::OsStr>,
    current_exe: Option<&std::ffi::OsStr>,
) -> bool {
    [argv0, current_exe].into_iter().flatten().any(|value| {
        let Some(name) = std::path::Path::new(value).file_name() else {
            return false;
        };
        #[cfg(windows)]
        {
            name.to_str().is_some_and(|name| {
                let lowercase = name.to_ascii_lowercase();
                lowercase.strip_suffix(".exe").unwrap_or(&lowercase) == "soulmate"
            })
        }
        #[cfg(not(windows))]
        {
            name == "soulmate"
        }
    })
}

pub(crate) fn is_exitbind() -> bool {
    profile().surface == Surface::Exitbind
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn only_the_exitbind_profile_is_current() {
        assert_eq!(EXITBIND.caller, "exitbind");
        assert_eq!(EXITBIND.product, "exitbind");
        assert_eq!(EXITBIND.format_version, 8);
        assert_eq!(EXITBIND.installed_commands, ["exitbind"]);
        assert_eq!(profile(), EXITBIND);
        assert_eq!(profile().installed_commands, ["exitbind"]);
    }

    #[test]
    fn retired_name_is_detected_from_copy_or_symlink_argv0() {
        assert!(invoked_as_retired_soulmate(
            Some(std::ffi::OsStr::new("soulmate")),
            None
        ));
        assert!(invoked_as_retired_soulmate(
            Some(std::ffi::OsStr::new("/tmp/alias/soulmate")),
            None
        ));
        assert!(invoked_as_retired_soulmate(
            Some(std::ffi::OsStr::new("exitbind")),
            Some(std::ffi::OsStr::new("/opt/bin/soulmate"))
        ));
        assert!(!invoked_as_retired_soulmate(
            Some(std::ffi::OsStr::new("exitbind")),
            Some(std::ffi::OsStr::new("/opt/bin/exitbind"))
        ));
        assert!(!invoked_as_retired_soulmate(None, None));
    }

    #[cfg(windows)]
    #[test]
    fn windows_retired_name_accepts_optional_extension_and_case_variants() {
        for name in ["soulmate", "SoulMate", "soulmate.exe", "SOULMATE.EXE"] {
            assert!(
                invoked_as_retired_soulmate(Some(std::ffi::OsStr::new(name)), None),
                "{name}"
            );
        }
        for name in ["exitbind", "exitbind.exe", "soulmate-copy.exe"] {
            assert!(
                !invoked_as_retired_soulmate(Some(std::ffi::OsStr::new(name)), None),
                "{name}"
            );
        }
    }

    #[test]
    fn matrix_keeps_the_typed_profile_projection_bound() {
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../../compatibility/rename-matrix.json")).unwrap();
        let current = matrix["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "current-exitbind")
            .unwrap();
        assert_eq!(current["callerBasename"], EXITBIND.caller);
        assert_eq!(current["product"], EXITBIND.product);
        assert_eq!(
            current["installedCommands"],
            serde_json::json!(EXITBIND.installed_commands)
        );
        let path = matrix["paths"]
            .as_array()
            .unwrap()
            .iter()
            .find(|path| path["id"] == current["pathId"])
            .unwrap();
        assert_eq!(path["defaultConfig"], EXITBIND.config);
        assert_eq!(path["defaultControl"], EXITBIND.control);
        assert_eq!(path["defaultState"], EXITBIND.state);
        assert_eq!(matrix["surfaces"][1]["status"], "historical-only");
    }

    #[test]
    fn profile_path_fields_are_relative_names() {
        for path in [
            EXITBIND.config,
            EXITBIND.control,
            EXITBIND.state,
            "soulmate.json",
        ] {
            assert!(!Path::new(path).is_absolute());
        }
    }
}
