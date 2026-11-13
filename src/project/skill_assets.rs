// Defines shared skill assets and managed installation metadata.
// Recognizes the exact publication marker line; see `src/project/mod.rs`.
//! Shared optional skill assets and their installation metadata.

#[derive(Clone, Copy)]
pub(crate) struct Asset {
    pub(crate) relative: &'static str,
    pub(crate) content: &'static str,
    pub(crate) executable: bool,
}

pub(crate) const ASSETS: [Asset; 20] = [
    Asset {
        relative: "references/competence.md",
        content: include_str!("../../skills/exitbind/references/competence.md"),
        executable: false,
    },
    Asset {
        relative: "references/roles/lead.md",
        content: super::starter_profiles::PROFILES[0].1,
        executable: false,
    },
    Asset {
        relative: "references/roles/worker.md",
        content: super::starter_profiles::PROFILES[1].1,
        executable: false,
    },
    Asset {
        relative: "references/roles/reviewer.md",
        content: super::starter_profiles::PROFILES[2].1,
        executable: false,
    },
    Asset {
        relative: "resources/publication/README.md",
        content: include_str!("../../skills/exitbind/resources/publication/README.md"),
        executable: false,
    },
    Asset {
        relative: "resources/publication/pre-push",
        content: include_str!("../../skills/exitbind/resources/publication/pre-push"),
        executable: true,
    },
    Asset {
        relative: "resources/publication/check-public-text",
        content: include_str!("../../skills/exitbind/resources/publication/check-public-text"),
        executable: true,
    },
    Asset {
        relative: "resources/publication/check-tracked-snapshot",
        content: include_str!("../../skills/exitbind/resources/publication/check-tracked-snapshot"),
        executable: true,
    },
    Asset {
        relative: "resources/judgments/assurance.md",
        content: include_str!("../../skills/exitbind/resources/judgments/assurance.md"),
        executable: false,
    },
    Asset {
        relative: "resources/judgments/context-lifecycle.md",
        content: include_str!("../../skills/exitbind/resources/judgments/context-lifecycle.md"),
        executable: false,
    },
    Asset {
        relative: "resources/judgments/product.md",
        content: include_str!("../../skills/exitbind/resources/judgments/product.md"),
        executable: false,
    },
    Asset {
        relative: "resources/judgments/quantitative.md",
        content: include_str!("../../skills/exitbind/resources/judgments/quantitative.md"),
        executable: false,
    },
    Asset {
        relative: "resources/judgments/workflow.md",
        content: include_str!("../../skills/exitbind/resources/judgments/workflow.md"),
        executable: false,
    },
    Asset {
        relative: "resources/analysis/README.md",
        content: include_str!("../../skills/exitbind/resources/analysis/README.md"),
        executable: false,
    },
    Asset {
        relative: "resources/analysis/rollout-metrics.py",
        content: include_str!("../../skills/exitbind/resources/analysis/rollout-metrics.py"),
        executable: true,
    },
    Asset {
        relative: "resources/verification/README.md",
        content: include_str!("../../skills/exitbind/resources/verification/README.md"),
        executable: false,
    },
    Asset {
        relative: "resources/verification/bind-target.py",
        content: include_str!("../../skills/exitbind/resources/verification/bind-target.py"),
        executable: true,
    },
    Asset {
        relative: "resources/workflows/continuity.md",
        content: include_str!("../../skills/exitbind/resources/workflows/continuity.md"),
        executable: false,
    },
    Asset {
        relative: "resources/workflows/dispatch.md",
        content: include_str!("../../skills/exitbind/resources/workflows/dispatch.md"),
        executable: false,
    },
    Asset {
        relative: "resources/economy/instructions.md",
        content: include_str!("../../skills/exitbind/resources/economy/instructions.md"),
        executable: false,
    },
];

pub(crate) fn executable(relative: &str) -> bool {
    ASSETS
        .iter()
        .any(|asset| asset.executable && relative.ends_with(asset.relative))
}

pub(crate) fn publication_marker(bytes: &[u8]) -> bool {
    bytes.split(|byte| *byte == b'\n').any(|line| {
        line.strip_suffix(b"\r").unwrap_or(line) == b"# exitbind-managed-publication:v1"
    })
}

pub(crate) fn owner_can_execute(info: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        info.permissions().mode() & 0o100 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = info;
        true
    }
}

pub(crate) fn set_executable(file: &std::fs::File) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}
