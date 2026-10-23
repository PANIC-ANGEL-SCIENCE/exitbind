//! The typed route: the one value a route-carrying surface accepts. A route
//! is built here from its command words and the configuration it reads, and
//! leaves this module only through its renderers: the full argv, the bounded
//! argv with its same-executable and same-configuration flags, the argv
//! suffix, and the shell line. A surface that takes a [`Route`] cannot be
//! handed a command string built elsewhere.

use serde_json::{json, Value};
use std::path::Path;

/// A command the product prints for a role to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Route {
    /// The executable a printed form names; `None` is the one this process
    /// runs as.
    executable: Option<String>,
    words: Vec<String>,
    /// `Some` when the route ends in `--config` and a configuration path.
    config: Option<Config>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Config {
    /// The path as an argv word; `None` when it is not UTF-8.
    exact: Option<String>,
    /// The path as shell text, lossy when it is not UTF-8.
    display: String,
}

/// A route rendered within a byte budget: the longest argv that fits, and
/// whether the reader must add the executable or the configuration.
#[derive(Clone, Debug)]
pub(crate) struct Bounded {
    pub(crate) argv: Value,
    pub(crate) same_config: bool,
    pub(crate) same_executable: bool,
}

impl Bounded {
    /// `{"argv", "sameConfigRequired", "sameExecutableRequired"}`.
    pub(crate) fn json(&self) -> Value {
        json!({"argv": self.argv, "sameConfigRequired": self.same_config,
            "sameExecutableRequired": self.same_executable})
    }

    /// The same object with the argv under `command`.
    pub(crate) fn command_json(&self) -> Value {
        json!({"command": self.argv, "sameConfigRequired": self.same_config,
            "sameExecutableRequired": self.same_executable})
    }
}

impl Route {
    /// A route of these command words, run by this process's executable.
    pub(crate) fn new<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            executable: None,
            words: words.into_iter().map(Into::into).collect(),
            config: None,
        }
    }

    /// A route read back from an argv this module rendered, with the
    /// executable it names first. Only the fill route and the bounded views
    /// re-render a printed route; nothing else parses one.
    pub(crate) fn printed(argv: &[String]) -> Result<Self, String> {
        let (executable, words) = argv.split_first().ok_or("printed route is empty")?;
        Ok(Self {
            executable: Some(executable.clone()),
            words: words.to_vec(),
            config: None,
        })
    }

    /// A printed suffix (no executable) read back to be bounded again; it
    /// ends with `--config` and takes `config` as its value.
    pub(crate) fn reprinted(mut suffix: Vec<String>, config: &Path) -> Result<Self, String> {
        if suffix.last().map(String::as_str) != Some("--config") {
            return Err("printed route does not end with --config".into());
        }
        suffix.pop();
        Ok(Self::new(suffix).config(config))
    }

    /// Name `executable`, one this process resolved through
    /// [`super::executable`] or [`super::canonical_executable`], so a batch of
    /// routes printed together names the same executable.
    pub(crate) fn run_by(mut self, executable: &str) -> Self {
        self.executable = Some(executable.to_owned());
        self
    }

    /// Name the canonical path of the executable this process runs as.
    pub(crate) fn canonical(self) -> Result<Self, String> {
        Ok(self.run_by(&super::canonical_executable()?))
    }

    /// Append more command words.
    pub(crate) fn words<I, S>(mut self, words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.words.extend(words.into_iter().map(Into::into));
        self
    }

    /// End the route with `--config` and this configuration path.
    pub(crate) fn config(mut self, config: &Path) -> Self {
        self.config = Some(Config {
            exact: config.to_str().map(str::to_owned),
            display: config.display().to_string(),
        });
        self
    }

    /// End the route with `--config` and this configuration text.
    pub(crate) fn config_text(mut self, config: &str) -> Self {
        self.config = Some(Config {
            exact: Some(config.to_owned()),
            display: config.to_owned(),
        });
        self
    }

    fn executable(&self) -> Result<String, String> {
        match &self.executable {
            Some(executable) => Ok(executable.clone()),
            None => super::executable(),
        }
    }

    /// The command words without the executable, ending with `--config` when
    /// the route reads a configuration (its value is left to the reader).
    pub(crate) fn suffix(&self) -> Vec<String> {
        let mut suffix = self.words.clone();
        if self.config.is_some() {
            suffix.push("--config".into());
        }
        suffix
    }

    /// The complete argv: executable, words, `--config CONFIG`.
    pub(crate) fn argv(&self) -> Result<Vec<String>, String> {
        let mut argv = vec![self.executable()?];
        argv.extend(self.words.iter().cloned());
        if let Some(config) = &self.config {
            argv.push("--config".into());
            argv.push(
                config
                    .exact
                    .clone()
                    .ok_or("configuration path is not valid UTF-8")?,
            );
        }
        Ok(argv)
    }

    /// The argv as a refusal displays it: the executable, or a note that it
    /// is unavailable instead of a command name another executable may
    /// answer, the words, and the configuration path rendered lossily.
    pub(crate) fn display_argv(&self) -> Vec<String> {
        let mut argv = vec![self
            .executable()
            .unwrap_or_else(|_| "(executable unavailable)".to_owned())];
        argv.extend(self.words.iter().cloned());
        if let Some(config) = &self.config {
            argv.push("--config".into());
            argv.push(config.display.clone());
        }
        argv
    }

    /// The longest form of this route within `budget` JSON bytes: complete,
    /// then without the configuration value, then without the executable,
    /// then neither; the flags say what the reader must add back.
    pub(crate) fn bounded(&self, budget: usize) -> Bounded {
        let suffix = self.suffix();
        let config = self.config.as_ref().and_then(|config| config.exact.clone());
        let executable = self.executable().ok();
        let fits = |argv: &[String]| {
            serde_json::to_vec(argv)
                .expect("route argv is serializable")
                .len()
                <= budget
        };
        let bounded = |argv: Vec<String>, same_config: bool, same_executable: bool| Bounded {
            argv: json!(argv),
            same_config,
            same_executable,
        };
        if let Some(executable) = executable.as_ref() {
            let mut argv = vec![executable.clone()];
            argv.extend(suffix.iter().cloned());
            if let Some(config) = config.as_ref() {
                let mut complete = argv.clone();
                complete.push(config.clone());
                if fits(&complete) {
                    return bounded(complete, false, false);
                }
            }
            if fits(&argv) {
                return bounded(argv, true, false);
            }
        }
        if let Some(config) = config.as_ref() {
            let mut with_config = suffix.clone();
            with_config.push(config.clone());
            if fits(&with_config) {
                return bounded(with_config, false, true);
            }
        }
        bounded(suffix, true, true)
    }

    /// The route as one shell line: every word quoted as one shell word, the
    /// executable named as unavailable when it cannot be resolved.
    pub(crate) fn text(&self) -> String {
        let mut rest: Vec<String> = self.words.iter().map(|item| super::word(item)).collect();
        if let Some(config) = &self.config {
            rest.push("--config".into());
            rest.push(super::word(&config.display));
        }
        let rest = rest.join(" ");
        match &self.executable {
            Some(executable) => format!("{} {rest}", super::word(executable)),
            None => super::command_text(&rest),
        }
    }

    /// The route as a POSIX `exec` line for a generated script: the leading
    /// command words plain, a shell parameter (`$1`) double-quoted so the
    /// script passes its own argument, and every other value single-quoted.
    pub(crate) fn exec_line(&self) -> Result<String, String> {
        let quote = crate::presentation::shell_quote;
        let mut line = format!("exec {}", quote(&self.executable()?));
        let mut leading = true;
        for word in &self.words {
            let parameter =
                word.len() == 2 && word.starts_with('$') && word.as_bytes()[1].is_ascii_digit();
            leading &= !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-');
            line.push(' ');
            if parameter {
                line.push_str(&format!("\"{word}\""));
            } else if leading {
                line.push_str(word);
            } else {
                line.push_str(&quote(word));
            }
        }
        if let Some(config) = &self.config {
            let exact = config
                .exact
                .as_deref()
                .ok_or("configuration path is not valid UTF-8")?;
            line.push_str(&format!(" --config {}", quote(exact)));
        }
        Ok(line)
    }

    /// The process this route runs, for a route filled to run as printed.
    pub(crate) fn process(&self) -> Result<std::process::Command, String> {
        let argv = self.argv()?;
        let mut command = std::process::Command::new(&argv[0]);
        command.args(&argv[1..]);
        Ok(command)
    }
}

/// An unavailable route: nothing to run, and the reader must add both the
/// executable and the configuration.
pub(crate) fn unavailable() -> Bounded {
    Bounded {
        argv: Value::Null,
        same_config: true,
        same_executable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_renders_complete_then_drops_config_then_executable_within_budget() {
        let route = Route::new(["work", "next", "smw_1"]).config_text("/c/exitbind.json");
        let exe = super::super::executable().unwrap();
        let full = route.argv().unwrap();
        assert_eq!(full[0], exe);
        assert_eq!(
            &full[1..],
            ["work", "next", "smw_1", "--config", "/c/exitbind.json"]
        );
        let wide = route.bounded(16 * 1024);
        assert_eq!(wide.argv, json!(full));
        assert!(!wide.same_config && !wide.same_executable);
        let narrow = route.bounded(10);
        assert_eq!(narrow.argv, json!(["work", "next", "smw_1", "--config"]));
        assert!(narrow.same_config && narrow.same_executable);
        assert!(route
            .text()
            .ends_with(" work next smw_1 --config /c/exitbind.json"));
        assert_eq!(
            Route::reprinted(route.suffix(), Path::new("/c/exitbind.json")).unwrap(),
            route
        );
    }

    #[test]
    fn an_exec_line_keeps_command_words_plain_and_passes_shell_parameters() {
        let route = Route::new(["work", "file", "$1", "abc", "$2"])
            .config_text("/c/x.json")
            .run_by("/bin/x");
        assert_eq!(
            route.exec_line().unwrap(),
            "exec '/bin/x' work file \"$1\" 'abc' \"$2\" --config '/c/x.json'"
        );
    }

    #[test]
    fn a_printed_route_keeps_the_executable_it_names() {
        let printed = ["/bin/x".to_owned(), "work".to_owned(), "a b".to_owned()];
        let route = Route::printed(&printed).unwrap();
        assert_eq!(route.argv().unwrap(), printed);
        assert_eq!(route.text(), "/bin/x work 'a b'");
        assert!(Route::printed(&[]).is_err());
    }
}
