//! Goal actions and their ownership admission at the CLI boundary.

macro_rules! goal_actions {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(super) enum GoalAction {
            $($variant),+
        }

        impl GoalAction {
            pub(super) const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub(super) fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }

            pub(super) fn parse(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|action| action.name() == name)
            }
        }
    };
}

goal_actions! {
    Quarantine => "quarantine",
    Recover => "recover",
    Prepare => "prepare",
    Require => "require",
    Cover => "cover",
    Assign => "assign",
    Incorporate => "incorporate",
    Close => "close",
    Usage => "usage",
    Adopt => "adopt",
    Status => "status",
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Admission {
    ProjectOrOwner,
    Owner,
    /// Adoption authorizes the target journal and an unowned current session.
    Adoption,
    ReadOnly,
}

impl GoalAction {
    pub(super) fn admission(self) -> Admission {
        match self {
            Self::Prepare
            | Self::Require
            | Self::Cover
            | Self::Assign
            | Self::Incorporate
            | Self::Close
            | Self::Usage => Admission::ProjectOrOwner,
            Self::Quarantine | Self::Recover => Admission::Owner,
            Self::Adopt => Admission::Adoption,
            Self::Status => Admission::ReadOnly,
        }
    }
}
