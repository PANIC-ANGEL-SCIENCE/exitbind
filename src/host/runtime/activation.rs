use serde_json::{json, Value};

/// Classify activation from consequence and promotion requirements. A count of
/// touched files is intentionally absent: harmless work can span many files,
/// while one consequential change can require governance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Activation {
    Direct,
    Governed,
    Blocked,
}

impl Activation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Governed => "governed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivationFacts {
    pub(crate) material_consequence: bool,
    pub(crate) promotion_required: bool,
    pub(crate) available: bool,
    pub(crate) activated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivationReason {
    NoMaterialConsequence,
    GovernanceAvailable,
    GovernanceUnavailable,
}

impl ActivationReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoMaterialConsequence => "no_material_consequence",
            Self::GovernanceAvailable => "governance_available",
            Self::GovernanceUnavailable => "governance_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivationAssessment {
    pub(crate) activation: Activation,
    pub(crate) reason: ActivationReason,
    pub(crate) provenance: &'static str,
}

impl ActivationAssessment {
    pub(crate) fn value(self) -> Value {
        json!({
            "activation": self.activation.as_str(),
            "reason": self.reason.as_str(),
            "provenance": self.provenance,
        })
    }
}

pub(crate) fn assess_activation(facts: ActivationFacts) -> ActivationAssessment {
    let activation = classify_activation(
        facts.material_consequence,
        facts.promotion_required,
        facts.available,
        facts.activated,
    );
    let reason = match activation {
        Activation::Direct => ActivationReason::NoMaterialConsequence,
        Activation::Governed => ActivationReason::GovernanceAvailable,
        Activation::Blocked => ActivationReason::GovernanceUnavailable,
    };
    ActivationAssessment {
        activation,
        reason,
        provenance: "host_reported",
    }
}

#[allow(dead_code)]
pub(crate) fn classify_activation(
    material_consequence: bool,
    promotion_required: bool,
    available: bool,
    activated: bool,
) -> Activation {
    if !(material_consequence || promotion_required) {
        return Activation::Direct;
    }
    if available && activated {
        Activation::Governed
    } else {
        Activation::Blocked
    }
}
