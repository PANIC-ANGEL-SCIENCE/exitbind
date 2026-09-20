//! Typed deterministic core for bounded low-information iteration.
//!
//! Persistence, assignment binding, and artifact verification stay in the run
//! layer. This module owns only the transition table, so every adapter reaches
//! the same finite result without interpreting strings or provider output.

use serde::{Deserialize, Serialize};

pub(crate) const VERSION: u64 = 1;
pub(crate) const NO_INFORMATION_LIMIT: u64 = 2;
pub(crate) const POST_REPLAN_LIMIT: u64 = 1;
/// Compatibility name for the maximum number of consecutive no-information
/// mutations before evidence is mandatory. This is not a lifetime mutation
/// cap: exact new evidence resets the consecutive trajectory.
pub(crate) const HARD_ITERATION_BUDGET: u64 = NO_INFORMATION_LIMIT + POST_REPLAN_LIMIT;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Ready,
    ReplanRequired,
    EvidenceRequired,
    Blocked,
}

impl Phase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ReplanRequired => "replan_required",
            Self::EvidenceRequired => "evidence_required",
            Self::Blocked => "blocked",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ready" => Ok(Self::Ready),
            "replan_required" => Ok(Self::ReplanRequired),
            "evidence_required" => Ok(Self::EvidenceRequired),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("unknown governor phase: {value}")),
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::ReplanRequired => 1,
            Self::EvidenceRequired => 2,
            Self::Blocked => 3,
        }
    }

    fn at_least(self, minimum: Self) -> Self {
        if self.rank() >= minimum.rank() {
            self
        } else {
            minimum
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LoopState {
    pub(crate) phase: Phase,
    /// Monotonic telemetry. It never grants or removes permission.
    pub(crate) total_mutations: u64,
    pub(crate) no_information_streak: u64,
    pub(crate) replanned: bool,
    pub(crate) post_replan_no_information: u64,
}

impl Default for LoopState {
    fn default() -> Self {
        Self {
            phase: Phase::Ready,
            total_mutations: 0,
            no_information_streak: 0,
            replanned: false,
            post_replan_no_information: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Transition {
    Mutation { new_evidence: bool },
    MaterialReplan,
    NewEvidence,
    MutationRequested,
    SensorRequireReplan,
    SensorRequireEvidence,
    SensorBlock,
    SensorInert,
}

impl LoopState {
    /// Apply one semantic transition. Callers persist the typed event and the
    /// resulting phase atomically; an error means no event may be committed.
    pub(crate) fn apply(&mut self, transition: Transition) -> Result<(), String> {
        if self.phase == Phase::Blocked {
            return match transition {
                Transition::SensorInert => Ok(()),
                _ => Err("blocked governor cannot be reopened".into()),
            };
        }
        match transition {
            Transition::Mutation { new_evidence } => self.mutation(new_evidence),
            Transition::MaterialReplan => self.replan(),
            Transition::NewEvidence => {
                self.reset_consecutive_trajectory();
                Ok(())
            }
            Transition::MutationRequested => {
                if self.phase == Phase::EvidenceRequired {
                    self.phase = Phase::Blocked;
                }
                Ok(())
            }
            Transition::SensorRequireReplan => {
                self.phase = self.phase.at_least(Phase::ReplanRequired);
                Ok(())
            }
            Transition::SensorRequireEvidence => {
                self.phase = self.phase.at_least(Phase::EvidenceRequired);
                Ok(())
            }
            Transition::SensorBlock => {
                self.phase = Phase::Blocked;
                Ok(())
            }
            Transition::SensorInert => Ok(()),
        }
    }

    fn mutation(&mut self, new_evidence: bool) -> Result<(), String> {
        match self.phase {
            Phase::ReplanRequired => {
                return Err("material re-plan is required before the next mutation".into())
            }
            Phase::EvidenceRequired => {
                return Err("new exact evidence is required before the next mutation".into())
            }
            Phase::Blocked => return Err("blocked governor cannot be reopened".into()),
            Phase::Ready => {}
        }
        self.total_mutations = self.total_mutations.saturating_add(1);
        if new_evidence {
            self.reset_consecutive_trajectory();
            return Ok(());
        }
        if self.replanned {
            self.post_replan_no_information = self.post_replan_no_information.saturating_add(1);
            if self.post_replan_no_information >= POST_REPLAN_LIMIT {
                self.phase = Phase::EvidenceRequired;
            }
        } else {
            self.no_information_streak = self.no_information_streak.saturating_add(1);
            if self.no_information_streak >= NO_INFORMATION_LIMIT {
                self.phase = Phase::ReplanRequired;
            }
        }
        Ok(())
    }

    fn replan(&mut self) -> Result<(), String> {
        if self.phase != Phase::ReplanRequired {
            return Err("material re-plan is not currently required".into());
        }
        self.phase = Phase::Ready;
        self.no_information_streak = 0;
        self.replanned = true;
        self.post_replan_no_information = 0;
        Ok(())
    }

    fn reset_consecutive_trajectory(&mut self) {
        self.phase = Phase::Ready;
        self.no_information_streak = 0;
        self.replanned = false;
        self.post_replan_no_information = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_information_path_is_finite_and_evidence_can_recover_before_refusal() {
        let mut state = LoopState::default();
        state
            .apply(Transition::Mutation {
                new_evidence: false,
            })
            .unwrap();
        state
            .apply(Transition::Mutation {
                new_evidence: false,
            })
            .unwrap();
        assert_eq!(state.phase, Phase::ReplanRequired);
        state.apply(Transition::MaterialReplan).unwrap();
        state
            .apply(Transition::Mutation {
                new_evidence: false,
            })
            .unwrap();
        assert_eq!(state.phase, Phase::EvidenceRequired);

        let mut recovered = state;
        recovered.apply(Transition::NewEvidence).unwrap();
        assert_eq!(recovered.phase, Phase::Ready);
        assert_eq!(recovered.total_mutations, 3);

        state.apply(Transition::MutationRequested).unwrap();
        assert_eq!(state.phase, Phase::Blocked);
    }

    #[test]
    fn sensor_can_only_preserve_or_raise_conservatism() {
        let mut state = LoopState::default();
        state.apply(Transition::SensorRequireReplan).unwrap();
        assert_eq!(state.phase, Phase::ReplanRequired);
        state.apply(Transition::SensorInert).unwrap();
        assert_eq!(state.phase, Phase::ReplanRequired);
        state.apply(Transition::SensorRequireEvidence).unwrap();
        assert_eq!(state.phase, Phase::EvidenceRequired);
        state.apply(Transition::SensorRequireReplan).unwrap();
        assert_eq!(state.phase, Phase::EvidenceRequired);
        state.apply(Transition::SensorBlock).unwrap();
        assert_eq!(state.phase, Phase::Blocked);
    }

    #[test]
    fn exact_new_evidence_does_not_reset_lifetime_telemetry() {
        let mut state = LoopState::default();
        for _ in 0..5 {
            state
                .apply(Transition::Mutation { new_evidence: true })
                .unwrap();
        }
        assert_eq!(state.phase, Phase::Ready);
        assert_eq!(state.total_mutations, 5);
        assert_eq!(state.no_information_streak, 0);
    }
}
