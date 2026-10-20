//! What each command palette pick does (`Dashboard`'s `paletteCommands`
//! and `useChecksCommands`): the app runs checks and terminal commands
//! itself; the rest open a dialog or page the window's router still owns.

use super::running_checks::CheckRun;

/// A terminal pane command, as `useGpuiTerminalPane` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCommand {
  ToggleCollapse,
  ToggleMaximize,
  NewShell,
}

impl TerminalCommand {
  /// The pane's `Command.kind`.
  pub fn kind(self) -> &'static str {
    match self {
      TerminalCommand::ToggleCollapse => "toggleCollapse",
      TerminalCommand::ToggleMaximize => "toggleMaximize",
      TerminalCommand::NewShell => "newShell",
    }
  }
}

/// What picking palette command `id` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteCommand {
  /// `trust-repo-checks`.
  TrustChecks,
  /// `run-all-checks`: every workflow, reported once as "Checks".
  RunAllChecks,
  /// `run-check-<file>` and `rerun-setup-script`.
  Run(CheckRun),
  Terminal(TerminalCommand),
  /// A Dashboard handler that opens a dialog, a page or a flow.
  Route(String),
}

/// `paletteCommands[id] ?? checksCommands.find(id)`.
pub fn parse(id: &str) -> PaletteCommand {
  match id {
    "trust-repo-checks" => PaletteCommand::TrustChecks,
    "run-all-checks" => PaletteCommand::RunAllChecks,
    "rerun-setup-script" => PaletteCommand::Run(CheckRun::Setup),
    "toggle-terminal" => PaletteCommand::Terminal(TerminalCommand::ToggleCollapse),
    "maximize-terminal" => PaletteCommand::Terminal(TerminalCommand::ToggleMaximize),
    "new-shell-terminal" => PaletteCommand::Terminal(TerminalCommand::NewShell),
    id => match id.strip_prefix("run-check-") {
      Some(file) if !file.is_empty() => PaletteCommand::Run(CheckRun::Workflow(file.into())),
      _ => PaletteCommand::Route(id.into()),
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn runs_checks_and_terminal_commands_itself() {
    assert_eq!(parse("trust-repo-checks"), PaletteCommand::TrustChecks);
    assert_eq!(parse("run-all-checks"), PaletteCommand::RunAllChecks);
    assert_eq!(
      parse("run-check-ci.yml"),
      PaletteCommand::Run(CheckRun::Workflow("ci.yml".into()))
    );
    assert_eq!(
      parse("rerun-setup-script"),
      PaletteCommand::Run(CheckRun::Setup)
    );
    assert_eq!(
      parse("toggle-terminal"),
      PaletteCommand::Terminal(TerminalCommand::ToggleCollapse)
    );
    assert_eq!(
      parse("new-shell-terminal"),
      PaletteCommand::Terminal(TerminalCommand::NewShell)
    );
    assert_eq!(TerminalCommand::ToggleMaximize.kind(), "toggleMaximize");
  }

  #[test]
  fn leaves_dialogs_and_pages_to_the_router() {
    for id in [
      "dashboard",
      "settings",
      "open-repository",
      "switch-branch",
      "go-to-workspace",
      "delete-workspace",
      "open-stash",
      "run-check-",
    ] {
      assert_eq!(parse(id), PaletteCommand::Route(id.into()), "{id}");
    }
  }
}
