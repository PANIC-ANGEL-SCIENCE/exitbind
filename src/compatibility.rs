//! The small, typed boundary for the current and historical command surfaces.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Surface {
    Exitbind,
    Soulmate,
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

const EXITBIND_COMMANDS: &[&str] = &["exitbind"];
const SOULMATE_COMMANDS: &[&str] = &["soulmate"];

const EXITBIND: Profile = Profile {
    surface: Surface::Exitbind,
    caller: "exitbind",
    product: "exitbind",
    producer: "exitbind",
    format_version: 6,
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

const SOULMATE: Profile = Profile {
    surface: Surface::Soulmate,
    caller: "soulmate",
    product: "soulmate",
    producer: "soulmate",
    format_version: 1,
    config: "soulmate.json",
    control: "soulmate",
    state: ".soulmate",
    api: "https://api.github.com/repos/envy404/soulmate/releases?per_page=20",
    raw_installer: "https://raw.githubusercontent.com/envy404/soulmate/",
    install_prefix_env: "SOULMATE_INSTALL_PREFIX",
    cache: "soulmate/update.json",
    marker: "soulmate/update.lock",
    installed_commands: SOULMATE_COMMANDS,
};

pub(crate) fn profile() -> Profile {
    let is_exitbind = std::env::current_exe()
        .ok()
        .map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == EXITBIND.caller)
        })
        .unwrap_or(false);
    if is_exitbind {
        EXITBIND
    } else {
        SOULMATE
    }
}

pub(crate) fn is_exitbind() -> bool {
    profile().surface == Surface::Exitbind
}

#[cfg(test)]
fn legacy_release_asset(tag: &str) -> bool {
    let mut parts = tag.strip_prefix('v').unwrap_or_default().splitn(2, '.');
    let (Some(major), Some(rest)) = (parts.next(), parts.next()) else {
        return false;
    };
    if major != "0" {
        return false;
    }
    let Some((minor, rest)) = rest.split_once('.') else {
        return false;
    };
    if minor.is_empty() || (minor.len() > 1 && minor.starts_with('0')) {
        return false;
    }
    let Ok(minor) = minor.parse::<u64>() else {
        return false;
    };
    if minor > 16 {
        return false;
    }
    let patch_end = rest.find(['-', '+']).unwrap_or(rest.len());
    let patch = &rest[..patch_end];
    !patch.is_empty()
        && (patch == "0" || !patch.starts_with('0'))
        && patch.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn profiles_own_the_two_public_surfaces() {
        assert_eq!(EXITBIND.caller, "exitbind");
        assert_eq!(EXITBIND.product, "exitbind");
        assert_eq!(EXITBIND.format_version, 6);
        assert_eq!(EXITBIND.installed_commands, ["exitbind"]);
        assert_eq!(SOULMATE.caller, "soulmate");
        assert_eq!(SOULMATE.product, "soulmate");
        assert_eq!(SOULMATE.format_version, 1);
        assert_eq!(SOULMATE.installed_commands, ["soulmate"]);
    }

    #[test]
    fn historical_release_selector_rejects_malformed_semver() {
        assert!(legacy_release_asset(&format!("v{}.{}.{}", 0, 16, 0)));
        assert!(legacy_release_asset(&format!(
            "v{}.{}.{}-rc.2+build.7",
            0, 16, 999
        )));
        assert!(!legacy_release_asset(&format!("v{}.{}.{}", 0, 16, "00")));
        assert!(!legacy_release_asset(&format!("v{}.{}.{}", 0, 17, 0)));
        assert!(!legacy_release_asset(&format!("v{}.{}", 0, 16)));
    }

    #[test]
    fn matrix_keeps_the_typed_profile_projection_bound() {
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../compatibility/rename-matrix.json")).unwrap();
        for (id, expected) in [
            ("current-exitbind", EXITBIND),
            ("legacy-soulmate", SOULMATE),
        ] {
            let row = matrix["surfaces"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            assert_eq!(row["callerBasename"], expected.caller);
            assert_eq!(row["product"], expected.product);
            assert_eq!(
                row["installedCommands"],
                serde_json::json!(expected.installed_commands)
            );
            let path = matrix["paths"]
                .as_array()
                .unwrap()
                .iter()
                .find(|path| path["id"] == row["pathId"])
                .unwrap();
            assert_eq!(path["defaultConfig"], expected.config);
            assert_eq!(path["defaultControl"], expected.control);
            assert_eq!(path["defaultState"], expected.state);
        }
    }

    #[test]
    fn profile_path_fields_are_relative_names() {
        for path in [
            EXITBIND.config,
            EXITBIND.control,
            EXITBIND.state,
            SOULMATE.config,
        ] {
            assert!(!Path::new(path).is_absolute());
        }
    }
}
