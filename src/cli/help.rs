pub(super) fn scoped_help(command: &str, positional: &[String]) -> Option<String> {
    let path = std::iter::once(command)
        .chain(positional.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let usage = match path.as_slice() {
        ["work"] => {
            "work next WORK | work detail WORK | work closeout WORK | work continuation WORK | work resume [--history] | work focus WORK | work bind WORK ... | work child WORK ASSIGNMENT ... | work record WORK < JSON | work validate WORK --packet FILE | work expand WORK REFERENCE"
        }
        ["work", "begin"] => "work begin WORKFLOW --goal GOAL --check-command COMMAND [--review-policy required|omitted] [--basis JSON] (basis is a versioned JSON object; current actions provide exact argv)",
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
        ["project"] => "project context [memory ITEM_ID] [--json] | project agents [--apply] [--json] | project architecture [check] [--json]",
        ["project", "architecture", "select"] => "project architecture select SOURCE --decision reviewed --reason TEXT [--current-binding SHA --apply] [--json] [--config CONFIG] (preview returns the exact apply command)",
        ["project", "architecture"] | ["project", "architecture", "check"] => "project architecture [check] [--json] [--config CONFIG] | project architecture select SOURCE --decision reviewed --reason TEXT",
        ["memory", "revalidate"] => "memory revalidate LEAD LEDGER --from-config ORIGINAL_CONFIG --reason REVIEWED_REASON [--apply] [--json]",
        ["setup"] => "setup [--apply] [--json] --mode local|portable --root ROOT --scope lead,worker,reviewer [--observe PATHS] [--write PATHS] [--commands FACTUAL_COMMAND] [--check-command CHECK_COMMAND] [--goal GOAL] [--review-policy required|omitted] [--hosts codex,claude]",
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
        ["work", "usage"] => "work usage WORK [--json] [--config CONFIG] | work usage WORK --apply [--json] [--config CONFIG] < NUMERIC_JSON",
        ["work", "file-serve"] => "work file-serve SESSION --config CONFIG (host-owned stdio MCP transport)",
        ["work", "file"] => "work file prepare WORK ASSIGNMENT | work file read|refresh|edit|inspect SESSION PATH (native workers receive a bound edit tool)",
        ["work", "validate"] => "work validate WORK --packet FILE [--json]",
        ["work", "expand"] => "work expand WORK REFERENCE [--json]",
        ["work", "closeout"] => "work closeout WORK [--export | --output PATH | --receipt PATH] [--json] (read-only unless export is explicitly requested)",
        ["work", "detail"] => "work detail WORK [--json]",
        ["work", "resume"] => "work resume [--json] [--full]",
        ["work", "classify"] => {
            "work classify --material-consequence true|false --promotion-required true|false"
        }
        ["activity", "codex"] => {
            "activity codex [--model MODEL] [--reasoning-effort EFFORT] < PROMPT"
        }
        ["activity", "show"] => "activity show ACTIVITY_ID",
        ["run", "start"] => "run start WORKFLOW --goal GOAL --ledger LEDGER",
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
            "run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER"
        }
        ["goal", "incorporate"] => "goal incorporate --goal-id ID --goal TEXT",
        ["goal", "close"] => "goal close --goal-id ID --result-ref REF",
        ["goal", "status"] => "goal status [--themed]",
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
    Some(format!("Usage: {command_name} {usage}"))
}

#[cfg(test)]
mod tests {
    use super::scoped_help;

    #[test]
    fn scoped_work_help_keeps_the_subcommand_syntax() {
        let help = scoped_help("work", &["next".to_owned()]).unwrap();
        assert!(help.contains("Usage: "));
        assert!(help.contains("work next WORK"));
        let recovery = scoped_help("work", &["return".to_owned()]).unwrap();
        assert!(recovery.contains("--result-ref HELD_REFERENCE"));
    }

    #[test]
    fn scoped_read_help_exposes_redundant_json_routes() {
        let continuation = scoped_help("work", &["continuation".to_owned()]).unwrap();
        assert!(continuation.contains("--json"));
        let validate = scoped_help("work", &["validate".to_owned()]).unwrap();
        assert!(validate.contains("--json"));
        let expand = scoped_help("work", &["expand".to_owned()]).unwrap();
        assert!(expand.contains("--json"));
    }

    #[test]
    fn unknown_scoped_help_falls_back_to_global_help() {
        assert!(scoped_help("work", &["unknown".to_owned()]).is_none());
    }
}
