//! All integration tests in one binary, so cargo links `treq_lib` and its
//! dependencies once instead of once per file. `remote_*` (named by
//! `--test` in CI workflows and scripts) and
//! `core_workspaces_create_workspace_test` (sets `TREQ_APP_DATA_DIR` for the
//! whole process) stay separate binaries.

#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]

#[path = "../common/mod.rs"]
mod common;

mod auto_rebase_test;
mod checks_test;
mod cli_parallel_commit_test;
mod cli_process_test;
mod core_app_test;
mod core_changes_test;
mod core_commits_list_test;
mod core_commits_move_test;
mod core_commits_parent_guard_test;
mod core_commits_rewrite_scope_test;
mod core_commits_split_test;
mod core_commits_test;
mod core_commits_test_edit;
mod core_commits_timestamp_test;
mod core_git_head_sync_test;
mod core_jj_sync_status_test;
mod core_logging_test;
mod core_remote_dispatch_test;
mod core_repo_test;
mod core_resolve_test;
mod core_stash_test;
mod core_workspaces_archive_workspace_test;
mod core_workspaces_auto_push_test;
mod core_workspaces_commit_workspace_test;
mod core_workspaces_default_branch_sync_test;
mod core_workspaces_delete_workspace_test;
mod core_workspaces_files_test;
mod core_workspaces_list_workspace_statuses_test;
mod core_workspaces_merge_workspaces_test;
mod core_workspaces_move_workspace_changes_test;
mod core_workspaces_pr_stack_test;
mod core_workspaces_push_test;
mod core_workspaces_recreate_existing_test;
mod core_workspaces_snapshot_restore_test;
mod core_workspaces_sparse_checkout_test;
mod core_workspaces_stale_after_rebase_test;
mod core_workspaces_symlink_dirs_test;
mod core_workspaces_sync_after_commit_test;
mod core_workspaces_test;
mod core_workspaces_workspace_diff_test;
mod file_indexer_test;
mod jj_bookmark_track_test;
mod jj_home_git_head_sync_test;
mod jj_home_repo_target_test;
mod jj_import_git_refs_test;
mod jj_lib_vs_cli_test;
mod jj_restore_test;
mod pty_tests;
