pub(super) fn scoped_help(command: &str, positional: &[String]) -> Option<String> {
    let path = std::iter::once(command)
        .chain(positional.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let usage = match path.as_slice() {
        ["init"] => "init [--mode local|portable|auto] [--root ROOT] [--project-id ID] [--control-root ROOT] [--state-root ROOT] [--skip-skills | --refresh-skills] [--with-coffee] (creates project configuration and managed guidance; review placement and boundaries before use)",
        ["update"] => "update (checks for a stable release and may replace the current executable; an existing regular binary is required for rollback)",
        ["work"] => {
            "work next WORK | work detail WORK | work closeout WORK | work continuation WORK | work resume [--history] | work link-session WORK [--json] | work replace-check WORK ... | work focus WORK | work bind WORK ... | work child WORK ASSIGNMENT ... | work record WORK < JSON | work validate WORK --packet FILE | work expand WORK REFERENCE | work usage WORK [--json] | work <child> --help"
        }
        ["work", "begin"] => "work begin WORKFLOW --goal GOAL --check-command COMMAND [--review-policy required|omitted] [--pre-return-command COMMAND [--pre-return-timeout-ms MS] [--pre-return-capture-bytes BYTES]] [--detail] [--standalone] [--journey-integration --artifact CHECKER] [--goal-id ID --requirement IDs --artifact CHECKER [--scope integration] [--project-goal]] [--basis JSON] (an available native-host identity links ordinary Work context to the current conversation Goal; --standalone opts out; --journey-integration freezes accepted conversation tasks for a final-current checker; --project-goal selects a project-local named contract while retaining conversation context; --detail delivers current recipient context; after an interrupted native association, inspect status/detail and use work link-session only for the exact recorded admission and actual Work start; never repeat begin)",
        ["work", "link-session"] => "work link-session WORK [--config CONFIG] [--json] (recovers only a matching native-session Goal admission after verifying the actual immutable Work start; performs no Work, check, acceptance, or provider mutation)",
        ["work", "act"] => "work act WORK [--inspect] [--resume [--operation ASSIGNMENT]] [--model MODEL] [--reasoning-effort EFFORT] [--controlled-effects]",
        ["work", "next"] => "work next WORK [--json] [--full]",
        ["work", "continuation"] => {
            "work continuation WORK [--json] [--section NAME [--index N [--history-index N]]] [--config CONFIG]"
        }
        ["work", "record"] => "work record WORK [--config CONFIG] < JSON",
        ["work", "bind"] => {
            "work bind WORK --context TOKEN --host HOST --session SESSION --host-version VERSION"
        }
        ["work", "child"] => {
            "work child WORK ASSIGNMENT --context TOKEN --native-child ID [--result TEXT] < RESULT"
        }
        ["work", "child", "prepare"] => {
            "work child prepare WORK SHORT_ASSIGNMENT --context TOKEN [--agent-type NAME] [--perspectives JSON]"
        }
        ["work", "child", "context"] => "work child context WORK INTENT [--config CONFIG]",
        ["project"] => "project context [memory ITEM_ID] [--task GOAL] [--json] | project route --task TASK --purpose PURPOSE --role ROLE --host HOST [--agent NAME] [--json] | project agents [--apply] [--json] | project architecture [check] [--json]",
        ["project", "route"] => "project route --task TASK --purpose PURPOSE --role worker|reviewer|adviser --host codex|claude [--agent NAME] [--json] [--config CONFIG] (read-only configured eligibility and qualified agent-owned snapshot; ambiguity stays with the Lead; the native host owns execution)",
        ["project", "capabilities"] => "project capabilities [--task GOAL] [--host codex|claude] [--json] | project capabilities select DESCRIPTOR --decision reviewed --reason TEXT (preview emits exact apply; existing Work stays frozen; live refresh is pending next launch)",
        ["project", "capabilities", "select"] => "project capabilities select DESCRIPTOR --decision reviewed --reason TEXT [--current-binding SHA --apply] [--json] [--config CONFIG]",
        ["project", "context"] => "project context [memory ITEM_ID] [--task GOAL] [--json] [--config CONFIG] (task-specific lessons are selected only for the supplied relevant goal)",
        ["project", "architecture", "select"] => "project architecture select SOURCE --decision reviewed --reason TEXT [--current-binding SHA --apply] [--json] [--config CONFIG] (preview returns the exact apply command)",
        ["project", "architecture"] | ["project", "architecture", "check"] => "project architecture [check] [--json] [--config CONFIG] | project architecture select SOURCE --decision reviewed --reason TEXT",
        ["memory", "form"] => "memory form [SOURCE] --scope project-lessons.v1|specialist.AGENT.v1|contextual.v1 [--json] [--config CONFIG] (SOURCE validates exact inactive lesson/specialist bytes without qualification; contextual.v1 accepts no SOURCE and describes the configured task-context shape; lessons: 2048 source bytes/512 fact bytes; specialist: 4096 source bytes, guards are path/sha256 objects, prose belongs in preconditions)",
        ["memory", "choose"] => "memory choose AGENT [ITEM_ID ...] --task TASK --host codex|claude --reason LEAD_REASON [--previous DIGEST|absent] [--apply --expected-sha256 PREVIEW_DIGEST] [--json] [--config CONFIG] (version 2 contextual policy: machine-prepared task choice; zero items records explicit no-adoption; preview then exact replay-safe apply)",
        ["memory", "choice"] => "memory choice AGENT --task TASK [--request-id PREVIEW_DIGEST] [--json] [--config CONFIG] (read current choice or recover its exact recorded request, without launch or adoption)",
        ["memory", "candidates"] => "memory candidates AGENT [ITEM_ID] --task TASK --host codex|claude [--json] [--config CONFIG] (read-only bounded eligible metadata; ITEM_ID expands exact source data for Lead assessment, without activating guidance or changing rights)",
        ["memory", "propose"] => "memory propose AGENT SOURCE --scope SCOPE --ledger LEDGER --expires-at UTC_TIME [--config CONFIG] (project-lessons.v1: source <=2048 UTF-8 bytes, fact <=512; use memory form before proposal)",
        ["memory", "revalidate"] => "memory revalidate LEAD LEDGER --from-config ORIGINAL_CONFIG --reason REVIEWED_REASON [--apply] [--json]",
        ["memory", "correct-policy"] => "memory correct-policy LEDGER --from-config ORIGINAL_CONFIG --reason OWNER_REASON [--owner-decision APPROVED_DECISION --apply] [--json] [--config CONFIG] (preview returns the exact-item decision; apply records only the approved policy lineage and preserves source, expiry and Work pins)",
        ["setup"] => "setup [--apply] [--json] --mode local|portable --root ROOT --scope lead,worker,reviewer [--observe PATHS] [--write PATHS] [--commands FACTUAL_COMMAND] [--check-command CHECK_COMMAND] [--pre-return-command COMMAND [--pre-return-timeout-ms MS] [--pre-return-capture-bytes BYTES]] [--goal GOAL] [--review-policy required|omitted] [--hosts codex,claude]",
        ["work", "permit"] => "work permit WORK ASSIGNMENT --operation OPERATION [--request-id ID]",
        ["work", "replan"] => {
            "work replan WORK ASSIGNMENT [--hypothesis TEXT | --replan-file PATH|-]"
        }
        ["work", "evidence"] => "work evidence WORK ASSIGNMENT --artifact PATH",
        ["work", "sensor-request"] => "work sensor-request WORK ASSIGNMENT",
        ["work", "sensor-result"] => {
            "work sensor-result WORK ASSIGNMENT --assessment VALUE --input-digest HEX"
        }
        ["work", "return"] => {
            "work return WORK ASSIGNMENT --outcome OUTCOME [--reason TEXT] [--result-ref HELD_REFERENCE] [--current-binding BINDING] [--json]"
        }
        ["work", "disposition"] => {
            "work disposition WORK ASSIGNMENT --decision repair|defer|reject|supersede --reason TEXT [--repair-boundary TEXT --regression TEXT] [--successor-basis JSON] [--current-binding BINDING] (repair and supersede require both repair terms)"
        }
        ["work", "check"] => "work check WORK [--timeout-ms MS] (1..86400000; default1800000)",
        ["work", "recover-check"] => "work recover-check WORK [--apply --current-binding BINDING < APPROVED_JSON] [--json] (read-only preview or blocked unknown-termination diagnostic; configured Lead attests exact known-ended storage failure only; never reruns a check)",
        ["work", "replace-check"] => "work replace-check WORK (--check-command COMMAND | --pre-return-command COMMAND [--pre-return-timeout-ms MS] [--pre-return-capture-bytes BYTES]) --reason TEXT --classification defective-contract --current-binding BINDING [--apply] (previews by default; Lead-owned continuation preserves predecessor, final-check policy for a diagnostic replacement, and spent accounting)",
        ["work", "usage"] => "work usage WORK [--json] [--config CONFIG] | work usage WORK --apply [--json] [--config CONFIG] < NUMERIC_JSON",
        ["work", "file-serve"] => "work file-serve SESSION --config CONFIG (host-owned stdio MCP transport)",
        ["work", "file"] => "work file prepare WORK ASSIGNMENT | work file read|refresh|edit|inspect SESSION PATH (native workers receive a bound edit tool)",
        ["work", "validate"] => "work validate WORK --packet FILE [--json]",
        ["work", "expand"] => "work expand WORK REFERENCE [--json]",
        ["work", "closeout"] => "work closeout WORK [--export | --output PATH | --receipt PATH] [--json] (read-only unless export is explicitly requested)",
        ["work", "challenge"] => "work challenge [prepare] WORK --question QUESTION [--probe-command COMMAND --probe-source PROJECT_FILE] [--json] | work challenge validate WORK < REPORT_JSON (read-only bounded preparation/current-identity validation; no agents or probes launched)",
        ["work", "detail"] => "work detail WORK [--view action] [--json] (action gives current decision metadata, bound commands and complete section routes without repeating instructions)",
        ["work", "resume"] => "work resume [--json] [--full]",
        ["work", "classify"] => {
            "work classify --material-consequence true|false --promotion-required true|false"
        }
        ["activity", "codex"] => {
            "activity codex [--model MODEL] [--reasoning-effort EFFORT] < PROMPT"
        }
        ["activity", "show"] => "activity show ACTIVITY_ID",
        ["run", "start"] => "run start WORKFLOW --goal GOAL --ledger LEDGER [--pre-return-command COMMAND [--pre-return-timeout-ms MS] [--pre-return-capture-bytes BYTES]]",
        ["run", "pre-return", "read"] => "run pre-return read WORK_OR_LEDGER ATTEMPT_ID [--stream stdout|stderr] [--config CONFIG] (read-only retained diagnostic metadata or exact stored bytes; never executes a check)",
        ["run", "next"] => "run next LEDGER [--text]",
        ["run", "submit"] => "run submit AGENT LEDGER --outcome OUTCOME --artifact ARTIFACT",
        ["run", "review-policy"] => "run review-policy AGENT LEDGER --decision required|omitted",
        ["run", "record-check"] => {
            "run record-check LEDGER --target EVENT_SHA --check-command COMMAND --exit-code CODE"
        }
        ["run", "observe-check"] => "run observe-check LEDGER --target EVENT_SHA",
        ["run", "status"] => "run status LEDGER [--themed]",
        ["run", "inspect"] => "run inspect LEDGER [--event EVENT_SHA] [--json]",
        ["run", "report"] => "run report LEDGER [LEDGER ...]",
        ["run", "explain"] => "run explain LEDGER [--event EVENT_SHA] [--themed]",
        ["run", "supersede"] => {
            "run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER [--owner-recovery STATE_ARTIFACT_PATH]"
        }
        ["goal"] => "goal incorporate --goal-id ID --goal TEXT [--project] | goal prepare --goal-id ID --requirement IDS --artifact CHECKER --check-command COMMAND [--project] | goal require --goal-id ID --requirement ID --obligation EXACT_TEXT --artifact SOURCE [--project] | goal cover --goal-id ID [--project] | goal assign --goal-id ID --requirement IDs --result-ref WORK [--project] | goal status [--project] [--json] | goal close --goal-id ID --result-ref REF [--project] | goal usage --goal-id ID [--json] | goal <child> --help",
        ["goal", "prepare"] => "goal prepare --goal-id ID --requirement IDS --artifact PROJECT_CHECKER --check-command COMMAND [--scope integration] [--basis TEXT] [--project] [--json] (read-only named coverage and checker preparation before Work begin; emits a separate begin route)",
        ["goal", "incorporate"] => "goal incorporate --goal-id ID --goal TEXT [--project] (--project selects the configured project's legacy Goal journal; otherwise a valid native host identity selects its conversation Goal)",
        ["goal", "require"] => "goal require --goal-id ID --requirement ID --obligation EXACT_TEXT --artifact SOURCE [--project] [--json] (records or corrects an agreed requirement; correction retains history and invalidates old support)",
        ["goal", "cover"] => "goal cover --goal-id ID [--project] [--json] (Lead confirms the agreed finite set; this does not prove complete interpretation of natural language)",
        ["goal", "assign"] => "goal assign --goal-id ID --requirement IDs --result-ref WORK [--project] [--disposition add|replace] [--json] (IDs are comma-separated; replace retires old contributors for these IDs without removing requirements)",
        ["goal", "close"] => "goal close --goal-id ID [--result-ref REF] [--direct] [--project] [--view event --json] (when closure is current, event view returns compact completion; after a lost reply, read once with goal status --view event --json; direct completion has no EXIT READY)",
        ["goal", "status"] => "goal status [--project] [--json] [--themed] [--view event --json] (event view is read-only recovery for the same current completion after a lost close reply; under a native host identity this reads the conversation Goal unless --project is supplied)",
        ["goal", "usage"] => "goal usage --goal-id ID [--json] [--config CONFIG] | goal usage --goal-id ID --apply [--json] [--config CONFIG] < NUMERIC_JSON",
        ["context", "reduce"] => "context reduce --events FILE",
        ["context", "checkpoint"] => "context checkpoint --state FILE --proposal FILE",
        ["context", "sensor-request"] => "context sensor-request --state FILE",
        ["context", "seal"] => "context seal --event FILE",
        ["host"] => "host status [--hosts HOSTS] | host install [--hosts HOSTS] | host <child> --help",
        ["host", "status"] => "host status [--hosts HOSTS]",
        ["host", "install"] => "host install [--hosts HOSTS]",
        ["hooks", "plan"] => "hooks plan --hosts HOSTS",
        ["hooks", "apply"] => "hooks apply --hosts HOSTS",
        ["hooks", "status"] => "hooks status --hosts HOSTS",
        ["hooks", "remove"] => "hooks remove --hosts HOSTS",
        _ => return None,
    };
    let command_name = crate::compatibility::profile().caller;
    let mut help = format!("Usage: {command_name} {usage}");
    if matches!(path.as_slice(), ["work", "permit"]) {
        help.push_str(&format!(
            "\n\nOPERATION must contain non-whitespace text, contain no NUL, and use at most {} UTF-8 bytes. Example: --operation \"Update import parsing\". Obtain allowed:true before editing; a permit grants no host permission. Recover a lost response using the same request ID and the current Work readback.",
            crate::run::OPERATION_MAX_BYTES
        ));
    }
    if matches!(path.as_slice(), ["goal", "usage"] | ["work", "usage"]) {
        help.push_str("\n\nInspect an existing goal or Work; --apply records one prospective observation (JSON on stdin, at most 8192 bytes). It launches no model and does not change task acceptance.\nExample for unavailable root counters (replace id for each distinct observation):\n{\"id\":\"root-setup-1\",\"source\":\"host_reported\",\"scope\":\"root\",\"phase\":\"setup\",\"status\":\"missing\",\"semantics\":\"delta\",\"lifetime\":\"invocation\"}\nMissing counters stay unknown. For actual non-overlapping invocation deltas, use status observed and values with inputTokens, cachedInputTokens and outputTokens. Cached input is part of input; total is input plus output. Never derive tokens from bytes or assume provider snapshots are additive.\nCounter lifetimes, cumulative baselines and parent/child overlap: REFERENCE.md, prospective usage observations in the goal section.");
    }
    Some(help)
}

#[cfg(test)]
mod tests {
    use super::scoped_help;

    #[test]
    fn scoped_work_help_keeps_the_subcommand_syntax() {
        let help = scoped_help("work", &["next".to_owned()]).unwrap();
        assert!(help.contains("Usage: "));
        assert!(help.contains("work next WORK"));
        let begin = scoped_help("work", &["begin".to_owned()]).unwrap();
        assert!(begin.contains("conversation Goal"));
        assert!(begin.contains("--project-goal"));
        assert!(begin.contains("--standalone"));
        assert!(begin.contains("--journey-integration"));
        let recovery = scoped_help("work", &["return".to_owned()]).unwrap();
        assert!(recovery.contains("--result-ref HELD_REFERENCE"));
    }

    #[test]
    fn goal_help_explains_explicit_legacy_project_selection() {
        let help = scoped_help("goal", &["incorporate".to_owned()]).unwrap();
        assert!(help.contains("--project"));
        assert!(help.contains("conversation Goal"));
    }

    #[test]
    fn scoped_read_help_exposes_redundant_json_routes() {
        let continuation = scoped_help("work", &["continuation".to_owned()]).unwrap();
        assert!(continuation.contains("--json"));
        let validate = scoped_help("work", &["validate".to_owned()]).unwrap();
        assert!(validate.contains("--json"));
        let expand = scoped_help("work", &["expand".to_owned()]).unwrap();
        assert!(expand.contains("--json"));
        let supersede = scoped_help("run", &["supersede".to_owned()]).unwrap();
        assert!(supersede.contains("--owner-recovery STATE_ARTIFACT_PATH"));
    }

    #[test]
    fn init_and_update_help_are_scoped_and_non_mutating() {
        let init = scoped_help("init", &[]).unwrap();
        assert!(init.contains("Usage: "));
        assert!(init.contains("--mode local|portable|auto"));
        assert!(init.contains("review placement and boundaries"));

        let update = scoped_help("update", &[]).unwrap();
        assert!(update.contains("Usage: "));
        assert!(update.contains("checks for a stable release"));
        assert!(update.contains("may replace the current executable"));
    }

    #[test]
    fn unknown_scoped_help_falls_back_to_global_help() {
        assert!(scoped_help("work", &["unknown".to_owned()]).is_none());
    }
}
