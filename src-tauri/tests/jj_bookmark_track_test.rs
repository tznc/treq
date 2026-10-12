mod common;
use common::TestRepo;

#[test]
fn bookmark_track_errors_when_remote_bookmark_missing() {
  let repo = TestRepo::with_remote().expect("test repo with remote");

  let err = treq_lib::jj::jj_bookmark_track(&repo.repo_path, "no-such-bookmark", "origin")
    .expect_err("tracking a missing remote bookmark must fail");
  assert!(
    err.to_string().contains("not found"),
    "unexpected error: {err}"
  );
  assert!(
    !treq_lib::jj::is_bookmark_tracked(&repo.repo_path, "no-such-bookmark", "origin").unwrap()
  );
}

#[test]
fn bookmark_track_errors_when_remote_missing() {
  let repo = TestRepo::with_remote().expect("test repo with remote");
  let branch = repo.default_branch();

  let err = treq_lib::jj::jj_bookmark_track(&repo.repo_path, branch, "nonremote")
    .expect_err("tracking on a missing remote must fail");
  assert!(
    err.to_string().contains("not found"),
    "unexpected error: {err}"
  );
  assert!(!treq_lib::jj::is_bookmark_tracked(&repo.repo_path, branch, "nonremote").unwrap());
}

#[test]
fn bookmark_track_succeeds_for_existing_remote_bookmark() {
  let repo = TestRepo::with_remote().expect("test repo with remote");
  let branch = repo.default_branch();

  treq_lib::jj::jj_bookmark_track(&repo.repo_path, branch, "origin").expect("track");
  assert!(treq_lib::jj::is_bookmark_tracked(&repo.repo_path, branch, "origin").unwrap());
}
