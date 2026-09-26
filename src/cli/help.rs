pub(super) fn scoped_help(command: &str, positional: &[String]) -> Option<String> {
    let path = std::iter::once(command)
        .chain(positional.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let usage = match path.as_slice() {
        ["work"] => {
            "work next WORK | work continuation WORK | work resume | work record WORK < JSON"
        }
        ["work", "begin"] => "work begin WORKFLOW --goal GOAL --check-command COMMAND",
        ["work", "next"] => "work next WORK [--json] [--full]",
        ["work", "continuation"] => {
            "work continuation WORK [--section NAME [--index N [--history-index N]]] [--config CONFIG]"
        }
        ["work", "record"] => "work record WORK [--config CONFIG] < JSON",
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
            "work return WORK ASSIGNMENT --outcome OUTCOME [--result-ref HELD_REFERENCE] [--json]"
        }
        ["work", "check"] => "work check WORK",
        ["work", "validate"] => "work validate WORK --packet FILE",
        ["work", "expand"] => "work expand WORK REFERENCE",
        ["work", "resume"] => "work resume [--json] [--full]",
        ["work", "classify"] => {
            "work classify --material-consequence true|false --promotion-required true|false"
        }
        ["run", "start"] => "run start WORKFLOW --goal GOAL --ledger LEDGER",
        ["run", "next"] => "run next LEDGER [--text]",
        ["run", "submit"] => "run submit AGENT LEDGER --outcome OUTCOME --artifact ARTIFACT",
        ["run", "review-policy"] => "run review-policy AGENT LEDGER --decision required|omitted",
        ["run", "record-check"] => {
            "run record-check LEDGER --target EVENT_SHA --check-command COMMAND --exit-code CODE"
        }
        ["run", "observe-check"] => "run observe-check LEDGER --target EVENT_SHA",
        ["run", "status"] => "run status LEDGER",
        ["run", "inspect"] => "run inspect LEDGER [--event EVENT_SHA] [--json]",
        ["run", "report"] => "run report LEDGER [LEDGER ...]",
        ["run", "explain"] => "run explain LEDGER [--event EVENT_SHA]",
        ["run", "supersede"] => {
            "run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER"
        }
        ["goal", "incorporate"] => "goal incorporate --goal-id ID --goal TEXT",
        ["goal", "close"] => "goal close --goal-id ID --result-ref REF",
        ["goal", "status"] => "goal status",
        ["context", "reduce"] => "context reduce --events FILE",
        ["context", "checkpoint"] => "context checkpoint --state FILE --proposal FILE",
        ["context", "sensor-request"] => "context sensor-request --state FILE",
        ["context", "seal"] => "context seal --event FILE",
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
    fn unknown_scoped_help_falls_back_to_global_help() {
        assert!(scoped_help("work", &["unknown".to_owned()]).is_none());
    }
}
