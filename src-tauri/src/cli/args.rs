//! Parses `treq <command>` before Tauri starts.
//!
//! `tauri-plugin-cli` only parses once the app is built, and building the app
//! opens the GTK event loop: without a display the CLI panicked, and the exit
//! code handed to `AppHandle::exit` from `setup` never reached the process. So
//! the CLI builds the same clap command from the `plugins.cli` section of
//! `tauri.conf.json` and maps its matches to `Matches`, the same shape the
//! plugin produces. The types live here so the CLI builds without Tauri.

use std::collections::HashMap;

use clap::{Arg as ClapArg, ArgAction, ArgMatches, Command};
use serde::Deserialize;
use serde_json::Value;

/// One argument's value: a string, an array, a bool for a flag, or null.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArgData {
  pub value: Value,
  pub occurrences: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubcommandMatches {
  pub name: String,
  pub matches: Matches,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Matches {
  pub args: HashMap<String, ArgData>,
  pub subcommand: Option<Box<SubcommandMatches>>,
}

const TAURI_CONFIG: &str = include_str!("../../tauri.conf.json");

/// Exit status for a malformed invocation, matching clap's own convention.
pub const USAGE_ERROR_EXIT_CODE: i32 = 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArgConfig {
  name: String,
  short: Option<char>,
  description: Option<String>,
  #[serde(default)]
  takes_value: bool,
  #[serde(default)]
  multiple: bool,
  #[serde(default)]
  required: bool,
  index: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandConfig {
  description: Option<String>,
  #[serde(rename = "afterHelp")]
  after_help: Option<String>,
  #[serde(default)]
  args: Vec<ArgConfig>,
  #[serde(default)]
  subcommands: HashMap<String, CommandConfig>,
}

#[allow(
  clippy::expect_used,
  reason = "tauri.conf.json is embedded at build time and checked by tests"
)]
fn cli_config() -> CommandConfig {
  let config: Value = serde_json::from_str(TAURI_CONFIG).expect("tauri.conf.json is valid JSON");
  serde_json::from_value(config["plugins"]["cli"].clone())
    .expect("tauri.conf.json plugins.cli uses only the keys the CLI parser supports")
}

fn build_command(name: &str, config: &CommandConfig) -> Command {
  let mut command = Command::new(name.to_string());
  if let Some(description) = &config.description {
    command = command.about(description.clone());
  }
  if let Some(after_help) = &config.after_help {
    command = command.after_help(after_help.clone());
  }
  for arg in &config.args {
    // Numeric values like `-1` parse as values (then fail validation), not flags.
    let mut clap_arg = ClapArg::new(arg.name.clone())
      .required(arg.required)
      .allow_negative_numbers(arg.takes_value || arg.multiple);
    match arg.index {
      Some(index) => clap_arg = clap_arg.index(index),
      None => {
        clap_arg = clap_arg.long(arg.name.clone());
        if let Some(short) = arg.short {
          clap_arg = clap_arg.short(short);
        }
      }
    }
    if let Some(description) = &arg.description {
      clap_arg = clap_arg.help(description.clone());
    }
    clap_arg = clap_arg.action(if arg.multiple {
      ArgAction::Append
    } else if arg.takes_value {
      ArgAction::Set
    } else {
      ArgAction::Count
    });
    command = command.arg(clap_arg);
  }
  let mut subcommands: Vec<_> = config.subcommands.iter().collect();
  subcommands.sort_by(|a, b| a.0.cmp(b.0));
  for (sub_name, sub_config) in subcommands {
    command = command.subcommand(build_command(sub_name, sub_config));
  }
  command
}

/// Same value shapes `tauri-plugin-cli` produces: a string for a single value,
/// an array for a repeatable one, a bool for a flag, null when absent.
fn to_matches(config: &CommandConfig, matches: &ArgMatches) -> Matches {
  let mut result = Matches::default();
  for arg in &config.args {
    let mut data = ArgData::default();
    if arg.takes_value || arg.multiple {
      if arg.multiple {
        if let Some(values) = matches.get_many::<String>(&arg.name) {
          let values: Vec<Value> = values.map(|v| Value::String(v.clone())).collect();
          data.occurrences = values.len().min(u8::MAX as usize) as u8;
          data.value = Value::Array(values);
        }
      } else if let Some(value) = matches.get_one::<String>(&arg.name) {
        data.occurrences = 1;
        data.value = Value::String(value.clone());
      }
    } else {
      data.occurrences = matches.get_count(&arg.name);
      data.value = Value::Bool(data.occurrences > 0);
    }
    result.args.insert(arg.name.clone(), data);
  }
  if let Some((name, sub_matches)) = matches.subcommand() {
    if let Some(sub_config) = config.subcommands.get(name) {
      result.subcommand = Some(Box::new(SubcommandMatches {
        name: name.to_string(),
        matches: to_matches(sub_config, sub_matches),
      }));
    }
  }
  result
}

/// Parses `args` (including the program name) against the `treq` CLI.
/// `Err` carries clap's error, which also covers `--help` and `--version`.
pub fn parse<I, S>(args: I) -> Result<Matches, clap::Error>
where
  I: IntoIterator<Item = S>,
  S: Into<std::ffi::OsString> + Clone,
{
  let config = cli_config();
  let command = build_command("treq", &config).version(env!("CARGO_PKG_VERSION"));
  let matches = command.try_get_matches_from(args)?;
  Ok(to_matches(&config, &matches))
}

/// `treq <name> --help` text, for tests.
#[cfg(test)]
pub(super) fn subcommand_help(name: &str) -> String {
  let mut command = build_command("treq", &cli_config());
  command.build();
  let sub = command.find_subcommand_mut(name).expect("known subcommand");
  sub.render_long_help().to_string()
}

/// Runs one CLI invocation and returns the process exit status.
pub fn run<I, S>(args: I) -> i32
where
  I: IntoIterator<Item = S>,
  S: Into<std::ffi::OsString> + Clone,
{
  let matches = match parse(args) {
    Ok(matches) => matches,
    Err(error) => {
      // Prints help/version to stdout and usage errors to stderr.
      let _ = error.print();
      return if error.use_stderr() {
        USAGE_ERROR_EXIT_CODE
      } else {
        0
      };
    }
  };
  let Some(subcommand) = matches.subcommand else {
    eprintln!("{}", build_command("treq", &cli_config()).render_usage());
    return USAGE_ERROR_EXIT_CODE;
  };
  super::init_cli_binary_paths();
  super::handle_cli_command(&subcommand).unwrap_or_else(|| {
    eprintln!("Unknown command: {}", subcommand.name);
    USAGE_ERROR_EXIT_CODE
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_configured_command_parses() {
    let config = cli_config();
    assert!(config.subcommands.contains_key("st"));
    build_command("treq", &config).debug_assert();
  }

  #[test]
  fn maps_positional_single_multiple_and_flag_values() {
    let matches = parse(["treq", "add", "feat/x", "-k", "a", "-k", "b", "-d", "desc"]).unwrap();
    let sub = matches.subcommand.unwrap();
    assert_eq!(sub.name, "add");
    assert_eq!(sub.matches.args["branch_name"].value, Value::from("feat/x"));
    assert_eq!(sub.matches.args["description"].value, Value::from("desc"));
    assert_eq!(
      sub.matches.args["symlink"].value,
      Value::Array(vec![Value::from("a"), Value::from("b")])
    );
    assert_eq!(sub.matches.args["title"].value, Value::Null);

    let matches = parse(["treq", "commit", "feat/x", "-m", "msg", "--push"]).unwrap();
    let sub = matches.subcommand.unwrap();
    assert_eq!(sub.matches.args["push"].value, Value::Bool(true));
  }

  #[test]
  fn usage_errors_exit_with_code_two() {
    assert_eq!(run(["treq", "bogus"]), USAGE_ERROR_EXIT_CODE);
    assert_eq!(run(["treq", "commit", "feat/x"]), USAGE_ERROR_EXIT_CODE);
  }

  #[test]
  fn help_and_version_exit_zero() {
    assert_eq!(run(["treq", "--version"]), 0);
    assert_eq!(run(["treq", "--help"]), 0);
  }
}
