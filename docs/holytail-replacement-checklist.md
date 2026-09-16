# Holytail Replacement Checklist

This checklist describes the bounded Exitbind-owned path for preservation-risk
checked work. It does not authorize removing Holytail, publishing a release, or
claiming host-level enforcement.

Completion report:

```text
MECHANISM/PACKAGING VERIFIED; LIVE HOST UNVERIFIED
```

## Covered

| Claim | Evidence |
| --- | --- |
| Accepted preservation requirements are first-class checked-run input. | `src/run_value.rs` stores versioned preservation requirements, command hashes, and proof origin. |
| Ordinary functional checks cannot satisfy preservation by accident. | `src/run_exit.rs` emits distinct `preservation_missing` and `preservation_failed` reasons. |
| A worker cannot promote its own claim into preservation proof. | Preservation evidence is recorded only through requirement-bound check events. |
| Resume output separates reusable evidence from remaining obligations. | `src/work.rs` emits `stillValid`, `doNotRepeat`, and `remaining` residual fields. |
| Relevant subject changes require fresh checks. | `tests/subject_progress.rs` covers stale subject evidence and preservation recheck after rework. |
| The public agent surface stays small. | `skills/exitbind/SKILL.md` describes the Exitbind-owned preservation path without requiring standalone Holytail. |

## Deliberately Retired Presentation

| Presentation | Replacement |
| --- | --- |
| Standalone Holytail as a required user-facing install for this path. | Exitbind preservation requirements and checks inside the checked-run lifecycle. |
| Separate ceremonial progress reporting for ordinary work. | `Neuro` plus `Exitbind progress: N%.` for routine narration. |
| Summary-based resume instructions. | Versioned residual packets generated from recorded run state. |

## Unsupported Gap

| Gap | Status |
| --- | --- |
| Owner-approved Holytail retirement, uninstall, release, or tag publication. | Not performed here. |
| Live host interception or compliance proof. | Not claimed; host behavior remains externally verified. |
| Whole-machine fingerprinting or arbitrary checker dependency graphs. | Deliberately unsupported; Exitbind binds configured checks to the current subject. |
| Universal semantic equivalence between prose and implementation. | Not claimed; accepted requirements still need explicit checks and lead acceptance. |
