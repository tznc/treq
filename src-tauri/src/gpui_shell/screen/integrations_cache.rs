//! What the GitHub and Linear screens read last, kept past the view that
//! read it. A view opening on a key it has seen shows the old answer in the
//! first frame and reads again under it; every read writes back, so a
//! change made through the app (a comment, a close, a new issue) is shown
//! by the next read. Keys carry the repository, so one repository's rows
//! never stand in for another's.

use tauri_plugin_gpui::gpui::{App, Global};

use super::{github_data::StateFilter, prefetch::Lru};
use crate::{
  github::{GhIssue, GhListPage, GhPullRequest},
  linear::{LinearIssue, LinearUser},
};

/// A list's first page by repository and state filter.
pub(super) type ListKey = (String, StateFilter);

pub(super) struct Integrations {
  /// A repository path's GitHub `owner/name`, which the dialog needs to
  /// start listing pull requests.
  pub remote_names: Lru<String, String>,
  /// The New Workspaces dialog's pull requests by `owner/name` and search,
  /// and whether they are the user's own.
  pub open_prs: Lru<(String, String), (Vec<GhPullRequest>, bool)>,
  /// Linear issues by repository path and team, and the viewer by
  /// repository path; the dialog and the Linear view share them.
  pub linear_issues: Lru<(String, Option<String>), Vec<LinearIssue>>,
  pub linear_viewer: Lru<String, LinearUser>,
  pub issue_pages: Lru<ListKey, GhListPage<GhIssue>>,
  pub pr_pages: Lru<ListKey, GhListPage<GhPullRequest>>,
  /// Issue and pull request details by `owner/name` and number.
  pub issues: Lru<(String, u64), GhIssue>,
  pub prs: Lru<(String, u64), GhPullRequest>,
}

impl Default for Integrations {
  fn default() -> Self {
    Self {
      remote_names: Lru::new(8),
      open_prs: Lru::new(8),
      linear_issues: Lru::new(12),
      linear_viewer: Lru::new(4),
      issue_pages: Lru::new(12),
      pr_pages: Lru::new(12),
      issues: Lru::new(24),
      prs: Lru::new(24),
    }
  }
}

impl Global for Integrations {}

pub(super) fn cache(cx: &mut App) -> &mut Integrations {
  cx.default_global::<Integrations>()
}
