//! Steers agents in a Treq workspace from `git` to the treq CLI. A
//! workspace is a jj workspace with no `.git` of its own, so `git` there
//! finds the home repository: reads show the home repo's state and writes
//! change its working tree.

use serde_json::{json, Value};

/// Added to the agent's context when it runs `git` in a workspace.
pub const GIT_IN_WORKSPACE_REASON: &str = "This Treq workspace has no `.git` folder, so `git` \
here runs against the home repository: its output does not show this workspace's changes, and \
commands that write change the home repository. Use `treq st` instead of `git status`, `treq diff` \
(or `treq diff --stat`) instead of `git diff`, `jj log` instead of `git log`, `jj show <rev>` \
instead of `git show`, `jj file annotate <path>` instead of `git blame`, `jj restore <path>` \
instead of `git checkout -- <path>`, and `treq commit` (only when the user asks) instead of \
`git commit`.";

/// Words that run the next word as the command, or shell keywords that
/// can start a command list.
const PREFIX_WORDS: &[&str] = &[
  "command", "exec", "env", "sudo", "nohup", "time", "nice", "!", "{", "if", "then", "else",
  "elif", "do", "while", "until",
];

/// Shells whose `-c` argument is a command of its own.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash"];

/// Claude Code's PreToolUse hook for Bash: for a call that runs `git`, the
/// JSON that adds treq instructions to the agent's context. It makes no
/// permission decision, so the call still runs through the normal flow.
/// `None` for every other call.
pub fn pre_bash_hook_output(input: &str) -> Option<String> {
  let input: Value = serde_json::from_str(input).ok()?;
  let command = input.pointer("/tool_input/command")?.as_str()?;
  runs_git(command).then(|| {
    json!({
      "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "additionalContext": GIT_IN_WORKSPACE_REASON,
      }
    })
    .to_string()
  })
}

/// Whether any simple command in a shell command runs `git`.
pub fn runs_git(command: &str) -> bool {
  split_commands(command)
    .iter()
    .any(|words| simple_command_runs_git(words))
}

fn simple_command_runs_git(words: &[String]) -> bool {
  let mut rest = words
    .iter()
    .skip_while(|w| is_assignment(w) || w.starts_with('-') || PREFIX_WORDS.contains(&w.as_str()));
  let Some(program) = rest.next() else {
    return false;
  };
  let name = program.rsplit('/').next().unwrap_or(program);
  if name == "git" {
    return true;
  }
  if SHELLS.contains(&name) {
    let mut rest = rest.skip_while(|w| !is_command_flag(w));
    if rest.next().is_some() {
      return rest.next().is_some_and(|script| runs_git(script));
    }
  }
  false
}

/// `-c`, or combined short flags that include it, as in `-lc`.
fn is_command_flag(word: &str) -> bool {
  word.strip_prefix('-').is_some_and(|flags| {
    !flags.starts_with('-') && flags.contains('c') && flags.chars().all(|c| c.is_ascii_alphabetic())
  })
}

/// `NAME=value`, as in `GIT_PAGER=cat git log`.
fn is_assignment(word: &str) -> bool {
  word.split_once('=').is_some_and(|(name, _)| {
    name
      .chars()
      .next()
      .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
      && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
  })
}

/// The words of each simple command: split at `;`, `&`, `|`, newlines,
/// parentheses and backticks outside quotes, and at `$(` and backticks
/// inside double quotes, where the shell still runs what they hold.
/// Here-document bodies are text, not commands, so they are skipped.
fn split_commands(command: &str) -> Vec<Vec<String>> {
  let mut commands: Vec<Vec<String>> = vec![Vec::new()];
  let mut word: Option<String> = None;
  let mut quote: Option<char> = None;
  // Each open `(`, `$(` or backtick, with the quote to restore at its close.
  let mut nesting: Vec<(char, Option<char>)> = Vec::new();
  // Here-documents whose bodies start at the next newline: (delimiter, `<<-`).
  let mut heredocs: Vec<(String, bool)> = Vec::new();
  let mut chars = command.chars().peekable();

  fn end_word(word: &mut Option<String>, commands: &mut [Vec<String>]) {
    if let (Some(word), Some(command)) = (word.take(), commands.last_mut()) {
      command.push(word);
    }
  }
  fn next_command(word: &mut Option<String>, commands: &mut Vec<Vec<String>>) {
    end_word(word, commands);
    if commands.last().is_some_and(|c| !c.is_empty()) {
      commands.push(Vec::new());
    }
  }

  while let Some(c) = chars.next() {
    match (quote, c) {
      (Some('\''), '\'') => quote = None,
      (Some('\''), _) => word.get_or_insert_with(String::new).push(c),
      (Some('"'), '"') => quote = None,
      (_, '\\') => {
        if let Some(next) = chars.next() {
          word.get_or_insert_with(String::new).push(next);
        }
      }
      (Some('"'), '$') if chars.peek() == Some(&'(') => {
        chars.next();
        nesting.push(('(', quote.take()));
        next_command(&mut word, &mut commands);
      }
      (Some('"'), '`') => {
        nesting.push(('`', quote.take()));
        next_command(&mut word, &mut commands);
      }
      (Some(_), _) => word.get_or_insert_with(String::new).push(c),
      (None, '\'' | '"') => {
        quote = Some(c);
        word.get_or_insert_with(String::new);
      }
      (None, '`') => {
        if nesting.last().is_some_and(|(open, _)| *open == '`') {
          quote = nesting.pop().and_then(|(_, q)| q);
        } else {
          nesting.push(('`', None));
        }
        next_command(&mut word, &mut commands);
      }
      (None, '(') => {
        nesting.push(('(', None));
        next_command(&mut word, &mut commands);
      }
      (None, ')') => {
        if nesting.last().is_some_and(|(open, _)| *open == '(') {
          quote = nesting.pop().and_then(|(_, q)| q);
        }
        next_command(&mut word, &mut commands);
      }
      (None, '<') if chars.peek() == Some(&'<') => {
        end_word(&mut word, &mut commands);
        chars.next();
        // `<<<` is a here-string: its word follows on the same line.
        if chars.next_if_eq(&'<').is_some() {
          continue;
        }
        let strip_tabs = chars.next_if_eq(&'-').is_some();
        while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
        let mut delimiter = String::new();
        while let Some(c) = chars.next_if(|c| !c.is_whitespace() && !";&|<>()".contains(*c)) {
          if !matches!(c, '\'' | '"' | '\\') {
            delimiter.push(c);
          }
        }
        heredocs.push((delimiter, strip_tabs));
      }
      (None, '\n') => {
        next_command(&mut word, &mut commands);
        for (delimiter, strip_tabs) in heredocs.drain(..) {
          loop {
            let line: String = chars.by_ref().take_while(|c| *c != '\n').collect();
            let line = if strip_tabs {
              line.trim_start_matches('\t')
            } else {
              &line
            };
            if line == delimiter || chars.peek().is_none() {
              break;
            }
          }
        }
      }
      (None, ';' | '&' | '|') => next_command(&mut word, &mut commands),
      (None, c) if c.is_whitespace() => end_word(&mut word, &mut commands),
      (None, _) => word.get_or_insert_with(String::new).push(c),
    }
  }
  end_word(&mut word, &mut commands);
  commands
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn finds_git_in_any_part_of_a_command() {
    for command in [
      "git status",
      "git -C ../.. diff --stat",
      "/usr/bin/git log",
      "cd src && git diff",
      "make test; git status",
      "jj st || git status",
      "git log | head",
      "(cd .. && git status)",
      "echo $(git rev-parse --show-toplevel)",
      "echo \"$(git rev-parse HEAD)\" && ls",
      "echo `git branch`",
      "GIT_PAGER=cat git log",
      "command git status",
      "env -i git status",
      "sudo git checkout .",
      "bash -c 'git stash'",
      "sh -lc \"cd x && git reset --hard\"",
      "if true; then git status; fi",
      "echo \"$(pwd)\" && git status",
      "cat <<EOF > notes.md\ngit diff\nEOF\ngit status",
    ] {
      assert!(runs_git(command), "missed git in {command:?}");
    }
  }

  #[test]
  fn ignores_git_as_an_argument_or_in_quotes() {
    for command in [
      "jj git push",
      "gh pr diff 12",
      "echo git status",
      "grep -rn 'git diff; git status' docs",
      "echo \"run git status\"",
      "ls .git",
      "gitk",
      "cat .gitignore",
      "treq diff --stat",
      "cat > notes.md <<'EOF'\ngit status\nEOF",
      "treq set -d - <<-EOF\n\tgit diff\n\tEOF\nls",
      "grep x <<< 'git status'",
    ] {
      assert!(!runs_git(command), "flagged {command:?}");
    }
  }

  #[test]
  fn hook_adds_treq_instructions_without_blocking_a_git_call() {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"git diff --stat"},"cwd":"/ws"}"#;
    let output: serde_json::Value =
      serde_json::from_str(&pre_bash_hook_output(input).expect("instructions")).unwrap();
    let hook = &output["hookSpecificOutput"];
    assert_eq!(hook["hookEventName"], "PreToolUse");
    // No decision: the call runs through the normal permission flow.
    assert!(hook.get("permissionDecision").is_none(), "{hook}");
    let context = hook["additionalContext"].as_str().unwrap();
    assert!(context.contains("`treq diff`"), "{context}");
    assert!(context.contains("`treq st`"), "{context}");
  }

  #[test]
  fn hook_stays_silent_for_other_calls_and_bad_input() {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"treq diff"}}"#;
    assert_eq!(pre_bash_hook_output(input), None);
    assert_eq!(pre_bash_hook_output("not json"), None);
    assert_eq!(pre_bash_hook_output(r#"{"tool_input":{}}"#), None);
  }
}
