//! The GitHub panel's Merge Queue tab (`MergeQueueTab`): the rules of
//! `lib/merge-queue-stacks.ts` and the queue parts' labels and bars, and
//! what the tab reads: the queue from Supabase ([`crate::core::account`]),
//! and the workspaces' diff stats, the default branch and the target
//! branch's tip.

use std::collections::{HashMap, HashSet};

use super::{code_data::CodeHost, workspace_dialog_data::WorkspaceDialogHost};
use crate::core::account;
pub use crate::core::account::QueueEntry;

/// `MERGE_QUEUE_HISTORY_PAGE_SIZE`: merged entries each "Load more" adds.
pub const HISTORY_PAGE: usize = 5;
/// Merged entries read beyond the ones shown: the next "Load more" paints at
/// once, a stack cut at the edge of the read stays off screen, and the tip
/// marker still finds the newest landed stack's first entry.
pub const HISTORY_SLACK: usize = 20;
/// `DIFF_BAR_HALF_WIDTH_PX`.
pub const BAR_HALF: f32 = 14.;

/// What the tab reads from the backend; faked in tests. Blocks.
pub trait MergeQueueHost: CodeHost + WorkspaceDialogHost {
  /// Whether `full_name` opted in to the merge queue, and its entries in
  /// position order (none while it has not): the queued ones and the newest
  /// `history` merged ones.
  fn merge_queue(&self, full_name: &str, history: usize)
    -> Result<(bool, Vec<QueueEntry>), String>;
  /// Removes `branches` (a stack, bottom first) from the queue.
  fn dequeue(&self, full_name: &str, branches: &[String]) -> Result<(), String>;
}

impl MergeQueueHost for crate::gpui_shell::native::Backend {
  fn merge_queue(
    &self,
    full_name: &str,
    history: usize,
  ) -> Result<(bool, Vec<QueueEntry>), String> {
    let cloud = crate::commands::cloud::owned_cloud(self.app())
      .ok_or("The account is not available in this window.")?;
    crate::gpui_shell::block_on(async {
      let enabled = account::queue_enabled(&cloud.supabase, full_name).await?;
      let entries = if enabled {
        account::queue_entries(&cloud.supabase, full_name, Some(history)).await?
      } else {
        Vec::new()
      };
      Ok::<_, crate::core::supabase::ApiError>((enabled, entries))
    })
    .map_err(|e| e.to_string())
  }

  fn dequeue(&self, full_name: &str, branches: &[String]) -> Result<(), String> {
    let cloud = crate::commands::cloud::owned_cloud(self.app())
      .ok_or("The account is not available in this window.")?;
    crate::gpui_shell::block_on(account::dequeue_branches(
      &cloud.supabase,
      full_name,
      branches,
    ))
    .map_err(|e| e.to_string())
  }
}

/// Branches stacked on each other, in merge order (bottom first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueStack {
  pub target_branch: String,
  pub entries: Vec<QueueEntry>,
}

impl QueueStack {
  fn lowest(&self) -> i64 {
    self.entries.iter().map(|e| e.position).min().unwrap_or(0)
  }

  /// `isStackFullyMerged`.
  pub fn merged(&self) -> bool {
    !self.entries.is_empty() && self.entries.iter().all(|e| e.status == "merged")
  }
}

/// `buildQueueStacks`: a branch whose target is another queued branch is
/// stacked on it; stacks go by their lowest position.
pub fn build_stacks(entries: &[QueueEntry]) -> Vec<QueueStack> {
  let queued: HashSet<&str> = entries.iter().map(|e| e.branch_name.as_str()).collect();
  let mut children: HashMap<&str, Vec<&QueueEntry>> = HashMap::new();
  let mut roots: Vec<&QueueEntry> = Vec::new();
  for entry in entries {
    if queued.contains(entry.target_branch.as_str()) {
      children
        .entry(entry.target_branch.as_str())
        .or_default()
        .push(entry);
    } else {
      roots.push(entry);
    }
  }
  roots.sort_by_key(|e| e.position);
  let mut claimed: HashSet<&str> = HashSet::new();
  let mut stacks = Vec::new();
  for root in roots {
    let mut chain = Vec::new();
    let mut current = Some(root);
    while let Some(entry) = current.filter(|e| !claimed.contains(e.branch_name.as_str())) {
      claimed.insert(entry.branch_name.as_str());
      chain.push(entry.clone());
      // The lowest position wins if a branch is unexpectedly forked.
      current = children
        .get(entry.branch_name.as_str())
        .and_then(|c| c.iter().min_by_key(|e| e.position).copied());
    }
    stacks.push(QueueStack {
      target_branch: root.target_branch.clone(),
      entries: chain,
    });
  }
  // A leftover sits in a cycle with no root; show it rather than drop it.
  for entry in entries {
    if claimed.insert(entry.branch_name.as_str()) {
      stacks.push(QueueStack {
        target_branch: entry.target_branch.clone(),
        entries: vec![entry.clone()],
      });
    }
  }
  stacks.sort_by_key(QueueStack::lowest);
  stacks
}

/// `partitionQueueStacks`: stacks still queued, and fully merged ones,
/// newest first.
pub fn partition(entries: &[QueueEntry]) -> (Vec<QueueStack>, Vec<QueueStack>) {
  let (mut history, active): (Vec<_>, Vec<_>) = build_stacks(entries)
    .into_iter()
    .partition(QueueStack::merged);
  history.sort_by_key(|s| std::cmp::Reverse(s.lowest()));
  (active, history)
}

/// `takeHistoryPage`: the first stacks whose entries reach `limit`, and
/// whether more are left.
pub fn history_page(history: &[QueueStack], limit: usize) -> (Vec<QueueStack>, bool) {
  if limit == 0 {
    return (Vec::new(), !history.is_empty());
  }
  let mut visible = Vec::new();
  let mut count = 0;
  for stack in history {
    if count >= limit {
      break;
    }
    count += stack.entries.len();
    visible.push(stack.clone());
  }
  let total: usize = history.iter().map(|s| s.entries.len()).sum();
  (visible, count < total)
}

/// `mergeLocalDiffStats`: entries without lines changed take the local
/// workspace's.
pub fn with_local_stats(
  entries: &[QueueEntry],
  stats: &HashMap<String, (u32, u32)>,
) -> Vec<QueueEntry> {
  entries
    .iter()
    .map(|e| {
      let mut e = e.clone();
      if e.insertions.is_none() && e.deletions.is_none() {
        if let Some((i, d)) = stats.get(&e.branch_name) {
          e.insertions = Some(*i);
          e.deletions = Some(*d);
        }
      }
      e
    })
    .collect()
}

/// `queueStatusLabel`.
pub fn status_label(status: &str) -> &str {
  match status {
    "queued" => "Queued",
    "testing" => "Running checks",
    "merging" => "Merging",
    "merged" => "Merged",
    "failed" => "Failed",
    "dequeued" => "Dequeued",
    other => other,
  }
}

/// `stackMaxChange`.
pub fn max_change(entries: &[QueueEntry]) -> u32 {
  entries
    .iter()
    .map(|e| e.insertions.unwrap_or(0).max(e.deletions.unwrap_or(0)))
    .max()
    .unwrap_or(0)
}

/// `DiffBar`'s two widths, in pixels.
pub fn bar_widths(insertions: u32, deletions: u32, max: u32) -> (f32, f32) {
  let side = |n: u32| {
    if max == 0 || n == 0 {
      0.
    } else {
      ((n as f32 / max as f32) * BAR_HALF).round().max(1.)
    }
  };
  (side(insertions), side(deletions))
}

/// The target-branch marker's title: the last landed PR's.
pub fn tip_title(entry: Option<&QueueEntry>) -> Option<String> {
  let entry = entry?;
  match entry.pr_title.as_deref().map(str::trim) {
    Some(t) if !t.is_empty() => Some(t.to_string()),
    _ => entry.pr_number.map(|n| format!("PR #{n}")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(branch: &str, target: &str, position: i64, status: &str) -> QueueEntry {
    QueueEntry {
      branch_name: branch.into(),
      pr_number: Some(position as u64),
      pr_title: None,
      status: status.into(),
      position,
      target_branch: target.into(),
      insertions: None,
      deletions: None,
    }
  }

  fn branches(stack: &QueueStack) -> Vec<&str> {
    stack
      .entries
      .iter()
      .map(|e| e.branch_name.as_str())
      .collect()
  }

  #[test]
  fn stacks_follow_the_chain_and_order_by_lowest_position() {
    let entries = [
      entry("top", "mid", 3, "queued"),
      entry("solo", "main", 1, "queued"),
      entry("mid", "base", 4, "queued"),
      entry("base", "main", 2, "queued"),
    ];
    let stacks = build_stacks(&entries);
    assert_eq!(branches(&stacks[0]), ["solo"]);
    assert_eq!(branches(&stacks[1]), ["base", "mid", "top"]);
    assert_eq!(stacks[1].target_branch, "main");
  }

  #[test]
  fn cycles_are_kept_as_stacks_of_one() {
    let entries = [entry("a", "b", 1, "queued"), entry("b", "a", 2, "queued")];
    let stacks = build_stacks(&entries);
    assert_eq!(stacks.len(), 2);
  }

  #[test]
  fn fully_merged_stacks_go_to_history_newest_first() {
    let entries = [
      entry("old", "main", 1, "merged"),
      entry("new", "main", 5, "merged"),
      entry("part", "main", 2, "merged"),
      entry("part2", "part", 3, "testing"),
    ];
    let (active, history) = partition(&entries);
    assert_eq!(branches(&active[0]), ["part", "part2"]);
    assert_eq!(branches(&history[0]), ["new"]);
    assert_eq!(branches(&history[1]), ["old"]);
  }

  #[test]
  fn history_pages_by_entry_count() {
    let stack = |b: &str, n: usize| QueueStack {
      target_branch: "main".into(),
      entries: (0..n)
        .map(|i| entry(&format!("{b}{i}"), "main", 1, "merged"))
        .collect(),
    };
    let history = [stack("a", 3), stack("b", 3), stack("c", 1)];
    let (visible, more) = history_page(&history, 5);
    assert_eq!(visible.len(), 2);
    assert!(more);
    let (visible, more) = history_page(&history, 10);
    assert_eq!(visible.len(), 3);
    assert!(!more);
    assert_eq!(history_page(&history, 0), (Vec::new(), true));
  }

  #[test]
  fn local_stats_fill_entries_without_any() {
    let mut known = entry("k", "main", 1, "queued");
    known.insertions = Some(9);
    let stats: HashMap<String, (u32, u32)> = [("k".into(), (1, 1)), ("u".into(), (4, 2))].into();
    let out = with_local_stats(&[known, entry("u", "main", 2, "queued")], &stats);
    assert_eq!((out[0].insertions, out[0].deletions), (Some(9), None));
    assert_eq!((out[1].insertions, out[1].deletions), (Some(4), Some(2)));
    assert_eq!(max_change(&out), 9);
  }

  #[test]
  fn bars_scale_against_the_largest_change() {
    assert_eq!(bar_widths(10, 5, 10), (14., 7.));
    assert_eq!(bar_widths(1, 0, 100), (1., 0.));
    assert_eq!(bar_widths(3, 3, 0), (0., 0.));
  }

  #[test]
  fn labels_and_tip_titles() {
    assert_eq!(status_label("testing"), "Running checks");
    assert_eq!(status_label("weird"), "weird");
    let mut e = entry("a", "main", 7, "merged");
    assert_eq!(tip_title(Some(&e)), Some("PR #7".into()));
    e.pr_title = Some(" Fix ".into());
    assert_eq!(tip_title(Some(&e)), Some("Fix".into()));
    assert_eq!(tip_title(None), None);
  }

  /// Round 7: what one render used to rebuild (`with_local_stats` and
  /// `partition` over every entry) at the size of a long history, against the
  /// entries now read. `cargo test --lib queue_perf_derive -- --ignored --nocapture`.
  #[test]
  #[ignore]
  fn queue_perf_derive() {
    for (label, merged) in [
      ("10000 merged (before)", 10_000),
      ("25 merged + slack (after, once per change)", 45),
    ] {
      let entries: Vec<_> = (0..merged)
        .map(|i| entry(&format!("b{i}"), "main", i, "merged"))
        .collect();
      let stats = HashMap::new();
      let start = std::time::Instant::now();
      for _ in 0..20 {
        let all = with_local_stats(&entries, &stats);
        std::hint::black_box(partition(&all));
      }
      println!("{label}: {:?} per derive", start.elapsed() / 20);
    }
  }
}
