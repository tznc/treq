//! Concurrent `treq commits create` processes on one workspace must not
//! fork the working-copy change or conflict the workspace bookmark.
#![cfg(desktop)]
mod common;

use common::TestRepo;
use std::process::{Command, Stdio};
use treq_lib::core::remote::TreqCommandRequest;

#[test]
fn parallel_commits_on_one_workspace_do_not_diverge() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("parw1").unwrap();
  let path = repo.workspace_full_path(&ws);
  let base = TestRepo::jj_commit_ids_in_revset(&path, "@-").unwrap();
  TestRepo::write_workspace_file(&path, "a.txt", "a\n").unwrap();

  let children: Vec<_> = (0..5)
    .map(|i| {
      let args = TreqCommandRequest::CreateCommit {
        repo: repo.repo_path.clone(),
        workspace: Some(ws.id.to_string()),
        message: format!("parallel {i}"),
        base_change_id: None,
        idempotency_key: format!("par-{i}"),
      }
      .cli_args()
      .unwrap();
      Command::new(env!("CARGO_BIN_EXE_treq"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
    })
    .collect();
  let mut succeeded = 0;
  for child in children {
    let out = child.wait_with_output().unwrap();
    let output = format!(
      "{}{}",
      String::from_utf8_lossy(&out.stderr),
      String::from_utf8_lossy(&out.stdout)
    );
    // A losing process sees a clean working copy; with an empty-commit guard it
    // fails with "nothing to commit", otherwise it is a no-op.
    if out.status.success() {
      succeeded += 1;
    } else {
      assert!(
        output.contains("nothing to commit"),
        "unexpected failure: {output}"
      );
    }
  }
  assert!(succeeded >= 1, "no commits create call succeeded");

  let all = TestRepo::jj_change_ids_in_revset(&path, "all()").unwrap();
  let unique: std::collections::HashSet<_> = all.iter().collect();
  assert_eq!(all.len(), unique.len(), "divergent change ids: {all:?}");
  let tips = TestRepo::jj_commit_ids_in_revset(&path, "parw1").unwrap();
  assert_eq!(tips.len(), 1, "bookmark is conflicted: {tips:?}");
  let new = TestRepo::jj_log_entries(&path, &format!("{}..parw1", base[0]), 10).unwrap();
  assert!(
    new.len() == 1 && !new[0].is_empty,
    "expected one commit: {new:?}"
  );
  assert_eq!(
    TestRepo::jj_commit_ids_in_revset(&path, "parw1-").unwrap(),
    base
  );
  let files = TestRepo::jj_files_at_revision(&path, "parw1").unwrap();
  assert!(
    files.iter().any(|f| f == "a.txt"),
    "a.txt not committed: {files:?}"
  );
}
