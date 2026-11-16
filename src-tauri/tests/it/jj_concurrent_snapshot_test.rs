use crate::common::TestRepo;
use std::path::Path;
use std::sync::{Arc, Barrier};
use treq_lib::jj;

fn op_heads(repo_path: &str) -> usize {
  std::fs::read_dir(Path::new(repo_path).join(".jj/repo/op_heads/heads"))
    .unwrap()
    .count()
}

/// Workspaces of one repo share its operation log: snapshots of sibling
/// workspaces at the same moment must not fork it into divergent heads.
#[test]
fn concurrent_snapshots_of_sibling_workspaces_do_not_diverge() {
  let repo = TestRepo::new().unwrap();
  let paths: Vec<String> = (0..4)
    .map(|i| {
      let ws = repo
        .create_workspace_simple(&format!("feat/snap-{i}"))
        .unwrap();
      repo.workspace_full_path(&ws)
    })
    .collect();

  for round in 0..10 {
    let barrier = Arc::new(Barrier::new(paths.len()));
    let threads: Vec<_> = paths
      .iter()
      .cloned()
      .map(|path| {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
          TestRepo::write_workspace_file(&path, "edit.txt", &format!("round {round}")).unwrap();
          barrier.wait();
          jj::jj_get_changed_files(&path).unwrap();
        })
      })
      .collect();
    for t in threads {
      t.join().unwrap();
    }
    assert_eq!(
      op_heads(&repo.repo_path),
      1,
      "op log diverged in round {round}"
    );
  }
}
