//! Background PR-status + CI polling for workspace sidebars / headers.
//!
//! The UI used to call `gh pr view` / `gh pr checks` on a React Query
//! interval from the WebView. With many workspaces (and ShowWorkspace's
//! CI poll) that flooded IPC with concurrent subprocess work and caused
//! periodic stutter. This module owns the polling: a single background thread
//! refreshes PR info (~60s) and CI rollups (~30s) for every watched repo's
//! workspace branches and serves results from an in-memory cache. Frontend
//! reads are cache-only and never shell out to `gh`.

use crate::github::{PrCiStatus, PrInfo};
use crate::lock_ext::LockExt;
use crate::result_ext::ResultExt;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

/// Default cadence for `gh pr view` (PR metadata).
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Default cadence for `gh pr checks` (CI rollups). Faster than PR metadata
/// because check status changes more often while a PR is open; still owned by
/// the background thread so the UI never blocks on `gh`.
pub const DEFAULT_CI_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// While no window is in front, both cadences are this many times slower:
/// nobody is looking, and coming to the front polls whatever is due at once.
pub const BACKGROUND_SLOWDOWN: u32 = 5;

/// Fetch PR info for `(repo_path, branch_name)`.
pub type PrFetchFn = Arc<dyn Fn(&str, &str) -> Result<Option<PrInfo>, String> + Send + Sync>;

/// Fetch rolled-up CI status for `(repo_path, branch_name)`.
/// Only invoked when the PR fetch returned `Some(_)`.
pub type CiFetchFn = Arc<dyn Fn(&str, &str) -> Result<Option<PrCiStatus>, String> + Send + Sync>;

/// PR info and CI rollup for each requested branch of a repo, fetched in one
/// call. Every requested branch is a key; `None` means it has no PR.
pub type BulkFetchResult = HashMap<String, (Option<PrInfo>, Option<PrCiStatus>)>;

/// Fetch PR info and CI for all of a repo's `branches` at once. An `Err`
/// makes the poller fall back to per-branch fetches.
pub type BulkFetchFn =
  Arc<dyn Fn(&str, &[String]) -> Result<BulkFetchResult, String> + Send + Sync>;

/// List workspace branch names that should be polled for a repo.
pub type BranchListFn = Arc<dyn Fn(&str) -> Result<Vec<String>, String> + Send + Sync>;

/// Callback fired after a repo's cache is refreshed (used to emit Tauri events).
pub type OnUpdateFn = Arc<
  dyn Fn(&str, &HashMap<String, Option<PrInfo>>, &HashMap<String, Option<PrCiStatus>>)
    + Send
    + Sync,
>;

/// Per-repo map of `branch_name -> Option<PrInfo>` (None = no open/known PR).
pub type RepoPrCache = HashMap<String, Option<PrInfo>>;

/// Per-repo map of `branch_name -> Option<PrCiStatus>` (None = no PR / no checks).
pub type RepoCiCache = HashMap<String, Option<PrCiStatus>>;

struct Inner {
  cache: Mutex<HashMap<String, RepoPrCache>>,
  ci_cache: Mutex<HashMap<String, RepoCiCache>>,
  watched: Mutex<HashSet<String>>,
  fetch: PrFetchFn,
  ci_fetch: CiFetchFn,
  bulk_fetch: Option<BulkFetchFn>,
  list_branches: BranchListFn,
  on_update: Mutex<Option<OnUpdateFn>>,
  poll_interval: Duration,
  ci_poll_interval: Duration,
  /// In-flight single-branch refreshes keyed by `repo_path\\0branch_name`.
  branch_refresh_inflight: Mutex<HashSet<String>>,
  /// Signaled to wake the background loop early (watch/refresh/shutdown).
  wake: (Mutex<()>, Condvar),
  /// Set while a wake should trigger an immediate poll of all watched repos.
  pending_wake: AtomicBool,
  /// No window is in front: polls run [`BACKGROUND_SLOWDOWN`] times slower.
  background: AtomicBool,
  /// Set when a window comes to the front, so the loop works out again what
  /// is due by the foreground cadence.
  recheck: AtomicBool,
  shutdown: AtomicBool,
  loop_started: AtomicBool,
}

impl Inner {
  /// The PR and CI cadences, slowed while in the background.
  fn intervals(&self) -> (Duration, Duration) {
    let slowdown = if self.background.load(Ordering::SeqCst) {
      BACKGROUND_SLOWDOWN
    } else {
      1
    };
    (
      self.poll_interval * slowdown,
      self.ci_poll_interval * slowdown,
    )
  }
}

/// Process-wide background PR status poller + cache.
pub struct PrStatusManager {
  inner: Arc<Inner>,
}

impl PrStatusManager {
  pub fn new(fetch: PrFetchFn, list_branches: BranchListFn) -> Self {
    Self::new_with_ci(fetch, Arc::new(|_, _| Ok(None)), list_branches)
  }

  pub fn new_with_ci(fetch: PrFetchFn, ci_fetch: CiFetchFn, list_branches: BranchListFn) -> Self {
    Self::new_with_interval(fetch, ci_fetch, list_branches, DEFAULT_POLL_INTERVAL)
  }

  pub fn new_with_interval(
    fetch: PrFetchFn,
    ci_fetch: CiFetchFn,
    list_branches: BranchListFn,
    poll_interval: Duration,
  ) -> Self {
    Self::new_with_intervals(
      fetch,
      ci_fetch,
      list_branches,
      poll_interval,
      DEFAULT_CI_POLL_INTERVAL,
    )
  }

  pub fn new_with_intervals(
    fetch: PrFetchFn,
    ci_fetch: CiFetchFn,
    list_branches: BranchListFn,
    poll_interval: Duration,
    ci_poll_interval: Duration,
  ) -> Self {
    Self {
      inner: Arc::new(Inner {
        cache: Mutex::new(HashMap::new()),
        ci_cache: Mutex::new(HashMap::new()),
        watched: Mutex::new(HashSet::new()),
        fetch,
        ci_fetch,
        bulk_fetch: None,
        list_branches,
        on_update: Mutex::new(None),
        poll_interval,
        ci_poll_interval,
        branch_refresh_inflight: Mutex::new(HashSet::new()),
        wake: (Mutex::new(()), Condvar::new()),
        pending_wake: AtomicBool::new(false),
        background: AtomicBool::new(false),
        recheck: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
        loop_started: AtomicBool::new(false),
      }),
    }
  }

  /// Convenience for tests that want a custom interval after `new`.
  pub fn with_interval(self, poll_interval: Duration) -> Self {
    self.with_intervals(poll_interval, DEFAULT_CI_POLL_INTERVAL)
  }

  /// Convenience for tests that want custom PR + CI intervals.
  pub fn with_intervals(self, poll_interval: Duration, ci_poll_interval: Duration) -> Self {
    let fetch = Arc::clone(&self.inner.fetch);
    let ci_fetch = Arc::clone(&self.inner.ci_fetch);
    let list_branches = Arc::clone(&self.inner.list_branches);
    let on_update = self.inner.on_update.lock_or_recover().clone();
    let mut mgr = Self::new_with_intervals(
      fetch,
      ci_fetch,
      list_branches,
      poll_interval,
      ci_poll_interval,
    );
    if let Some(bulk_fetch) = self.inner.bulk_fetch.clone() {
      mgr = mgr.with_bulk_fetch(bulk_fetch);
    }
    if let Some(cb) = on_update {
      mgr.set_on_update(cb);
    }
    mgr
  }

  /// Fetch each repo's PRs and CI in one call per poll instead of one per
  /// branch. Call before the poller starts; per-branch fetches remain the
  /// fallback when the bulk call fails.
  pub fn with_bulk_fetch(mut self, bulk_fetch: BulkFetchFn) -> Self {
    if let Some(inner) = Arc::get_mut(&mut self.inner) {
      inner.bulk_fetch = Some(bulk_fetch);
    } else {
      log::warn!("pr-status: bulk fetch set after the manager was shared; ignored");
    }
    self
  }

  pub fn set_on_update(&self, cb: OnUpdateFn) {
    *self.inner.on_update.lock_or_recover() = Some(cb);
  }

  /// Ensure the background poll loop is running (idempotent).
  pub fn ensure_started(&self) {
    if self
      .inner
      .loop_started
      .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
      .is_err()
    {
      return;
    }
    let inner = Arc::clone(&self.inner);
    thread::Builder::new()
      .name("pr-status-poller".into())
      .spawn(move || background_loop(inner))
      .map(drop)
      .unwrap_or_else(|e| log::error!("pr-status: failed to spawn poller thread: {e}"));
  }

  /// Start watching a repo. A newly watched repo triggers an immediate
  /// background refresh; re-watching one is a no-op, because GPUI reads call
  /// this on every render and a wake per call forced a full poll each time.
  pub fn watch_repo(&self, repo_path: &str) {
    let newly_watched = self
      .inner
      .watched
      .lock_or_recover()
      .insert(repo_path.to_string());
    self.ensure_started();
    if newly_watched {
      self.request_wake();
    }
  }

  /// Stop watching a repo. Cached entries are retained until overwritten.
  pub fn unwatch_repo(&self, repo_path: &str) {
    let mut watched = self.inner.watched.lock_or_recover();
    watched.remove(repo_path);
  }

  /// Stops watching every repo `keep` rejects, so a repo no window shows any
  /// more costs no `gh` calls.
  pub fn retain_watched(&self, keep: impl Fn(&str) -> bool) {
    self
      .inner
      .watched
      .lock_or_recover()
      .retain(|repo| keep(repo));
  }

  /// Synchronously refresh one repo (tests + post-create-PR invalidation).
  pub fn refresh_repo_now(&self, repo_path: &str) {
    poll_one_repo(&self.inner, repo_path);
  }

  /// Synchronously refresh CI only for branches that already have cached PR
  /// info. Does not re-run `gh pr view`. Used by the 30s CI poll cadence and
  /// tests.
  pub fn refresh_ci_now(&self, repo_path: &str) {
    poll_ci_only(&self.inner, repo_path);
  }

  /// Synchronously refresh PR info + CI for a single branch (one logical
  /// refresh; CI is skipped when there is no PR). Used when the user opens
  /// a workspace so the cache is not left stale until the next poll tick.
  pub fn refresh_branch_now(&self, repo_path: &str, branch_name: &str) {
    poll_one_branch(&self.inner, repo_path, branch_name);
  }

  /// Queue an out-of-band single-branch PR+CI refresh on a background
  /// thread. Returns immediately; results land in the cache and fire
  /// `on_update` when ready. Concurrent queues for the same
  /// `(repo, branch)` coalesce so opening a workspace twice does not
  /// double-shell `gh`.
  pub fn queue_branch_refresh(&self, repo_path: String, branch_name: String) {
    self.ensure_started();
    let key = format!("{repo_path}\0{branch_name}");
    {
      let mut inflight = self.inner.branch_refresh_inflight.lock_or_recover();
      if !inflight.insert(key.clone()) {
        return;
      }
    }
    let inner = Arc::clone(&self.inner);
    thread::Builder::new()
      .name("pr-status-branch-refresh".into())
      .spawn(move || {
        poll_one_branch(&inner, &repo_path, &branch_name);
        inner.branch_refresh_inflight.lock_or_recover().remove(&key);
      })
      .map(drop)
      .unwrap_or_else(|e| log::error!("pr-status: failed to spawn branch refresh thread: {e}"));
  }

  /// Overwrite a single branch entry (e.g. after an on-demand `gh pr view`).
  pub fn put_cached(&self, repo_path: &str, branch_name: &str, info: Option<PrInfo>) {
    let mut cache = self.inner.cache.lock_or_recover();
    cache
      .entry(repo_path.to_string())
      .or_default()
      .insert(branch_name.to_string(), info);
  }

  /// Overwrite a single branch CI entry (e.g. after an on-demand `gh pr checks`).
  pub fn put_cached_ci(&self, repo_path: &str, branch_name: &str, status: Option<PrCiStatus>) {
    let mut cache = self.inner.ci_cache.lock_or_recover();
    cache
      .entry(repo_path.to_string())
      .or_default()
      .insert(branch_name.to_string(), status);
  }

  /// Cache snapshot for a repo. Empty if never polled.
  pub fn list_cached(&self, repo_path: &str) -> RepoPrCache {
    self
      .inner
      .cache
      .lock_or_recover()
      .get(repo_path)
      .cloned()
      .unwrap_or_default()
  }

  /// CI cache snapshot for a repo. Empty if never polled.
  pub fn list_cached_ci(&self, repo_path: &str) -> RepoCiCache {
    self
      .inner
      .ci_cache
      .lock_or_recover()
      .get(repo_path)
      .cloned()
      .unwrap_or_default()
  }

  /// Cached PR for a branch. `None` if the branch has not been polled yet;
  /// `Some(None)` if polled and no PR exists; `Some(Some(info))` otherwise.
  pub fn get_cached(&self, repo_path: &str, branch_name: &str) -> Option<Option<PrInfo>> {
    self
      .inner
      .cache
      .lock_or_recover()
      .get(repo_path)?
      .get(branch_name)
      .cloned()
  }

  /// Cached CI for a branch. Same `None` / `Some(None)` / `Some(Some(_))`
  /// semantics as [`Self::get_cached`].
  pub fn get_cached_ci(&self, repo_path: &str, branch_name: &str) -> Option<Option<PrCiStatus>> {
    self
      .inner
      .ci_cache
      .lock_or_recover()
      .get(repo_path)?
      .get(branch_name)
      .cloned()
  }

  /// Whether any window is in front. In the background polls slow down by
  /// [`BACKGROUND_SLOWDOWN`]; back in front, what the foreground cadence
  /// says is due polls at once, so a returning user sees fresh statuses.
  pub fn set_background(&self, background: bool) {
    let was = self.inner.background.swap(background, Ordering::SeqCst);
    if was && !background {
      self.inner.recheck.store(true, Ordering::SeqCst);
      let _guard = self.inner.wake.0.lock_or_recover();
      self.inner.wake.1.notify_one();
    }
  }

  fn request_wake(&self) {
    self.inner.pending_wake.store(true, Ordering::SeqCst);
    self.inner.wake.1.notify_one();
  }

  /// Test helper: whether the given repo is currently watched.
  #[cfg(test)]
  pub fn is_watching(&self, repo_path: &str) -> bool {
    self.inner.watched.lock_or_recover().contains(repo_path)
  }

  /// Stop the background loop (tests). Production keeps it for process life.
  #[cfg(test)]
  pub fn shutdown(&self) {
    self.inner.shutdown.store(true, Ordering::SeqCst);
    self.inner.wake.1.notify_one();
  }
}

fn background_loop(inner: Arc<Inner>) {
  use std::time::Instant;

  let mut last_pr: Option<Instant> = None;
  let mut last_ci: Option<Instant> = None;

  loop {
    if inner.shutdown.load(Ordering::SeqCst) {
      break;
    }

    let now = Instant::now();
    let wake_requested = inner.pending_wake.swap(false, Ordering::SeqCst);
    inner.recheck.store(false, Ordering::SeqCst);
    let (poll_interval, ci_poll_interval) = inner.intervals();
    let pr_due = wake_requested
      || last_pr
        .map(|t| now.duration_since(t) >= poll_interval)
        .unwrap_or(true);
    let ci_due = wake_requested
      || last_ci
        .map(|t| now.duration_since(t) >= ci_poll_interval)
        .unwrap_or(true);

    if pr_due || ci_due {
      let repos: Vec<String> = inner.watched.lock_or_recover().iter().cloned().collect();
      for repo in &repos {
        if inner.shutdown.load(Ordering::SeqCst) {
          break;
        }
        if pr_due {
          // Full refresh also updates CI for branches with PRs.
          poll_one_repo(&inner, repo);
        } else {
          poll_ci_only(&inner, repo);
        }
      }
      let stamped = Instant::now();
      if pr_due {
        last_pr = Some(stamped);
        last_ci = Some(stamped);
      } else if ci_due {
        last_ci = Some(stamped);
      }
    }

    if inner.shutdown.load(Ordering::SeqCst) {
      break;
    }

    // Sleep until the sooner of the next PR or CI deadline (or wake early).
    let now = Instant::now();
    let (poll_interval, ci_poll_interval) = inner.intervals();
    let until_pr = last_pr
      .map(|t| poll_interval.saturating_sub(now.duration_since(t)))
      .unwrap_or(Duration::ZERO);
    let until_ci = last_ci
      .map(|t| ci_poll_interval.saturating_sub(now.duration_since(t)))
      .unwrap_or(Duration::ZERO);
    let wait = until_pr.min(until_ci).max(Duration::from_millis(5));

    let (lock, cvar) = &inner.wake;
    let guard = lock.lock_or_recover();
    let (_guard, _timeout) = cvar
      .wait_timeout_while(guard, wait, |_| {
        !inner.pending_wake.load(Ordering::SeqCst)
          && !inner.recheck.load(Ordering::SeqCst)
          && !inner.shutdown.load(Ordering::SeqCst)
      })
      .unwrap_or_else(|poisoned| poisoned.into_inner());
  }
}

fn poll_one_repo(inner: &Inner, repo_path: &str) {
  let branches = match (inner.list_branches)(repo_path) {
    Ok(b) => b,
    Err(e) => {
      log::warn!("pr-status: failed to list branches for {repo_path}: {e}");
      return;
    }
  };

  let mut next: RepoPrCache = HashMap::new();
  let mut next_ci: RepoCiCache = HashMap::new();
  let bulk = inner.bulk_fetch.as_ref().and_then(|bulk_fetch| {
    bulk_fetch(repo_path, &branches)
      .inspect_err(|e| log::warn!("pr-status: bulk fetch failed for {repo_path}: {e}"))
      .ok()
  });
  for branch in branches {
    if let Some((pr_info, ci)) = bulk.as_ref().and_then(|bulk| bulk.get(&branch)) {
      next.insert(branch.clone(), pr_info.clone());
      next_ci.insert(branch, ci.clone());
      continue;
    }
    let pr_info = match (inner.fetch)(repo_path, &branch) {
      Ok(info) => {
        next.insert(branch.clone(), info.clone());
        info
      }
      Err(e) => {
        // Keep prior cache entry on transient gh failures so the UI
        // does not flicker to "no PR".
        log::warn!("pr-status: fetch failed for {repo_path}#{branch}: {e}");
        if let Some(prev) = inner
          .cache
          .lock_or_recover()
          .get(repo_path)
          .and_then(|m| m.get(&branch).cloned())
        {
          next.insert(branch.clone(), prev.clone());
          prev
        } else {
          None
        }
      }
    };

    fetch_ci_for_branch(inner, repo_path, &branch, pr_info.is_some(), &mut next_ci);
  }

  {
    let mut cache = inner.cache.lock_or_recover();
    cache.insert(repo_path.to_string(), next.clone());
  }
  {
    let mut ci_cache = inner.ci_cache.lock_or_recover();
    ci_cache.insert(repo_path.to_string(), next_ci.clone());
  }

  if let Some(cb) = inner.on_update.lock_or_recover().as_ref() {
    cb(repo_path, &next, &next_ci);
  }
}

/// Refresh CI using the existing PR cache (no `gh pr view`). Falls back to a
/// full poll when the PR cache is empty so the first CI cycle is not a no-op.
fn poll_ci_only(inner: &Inner, repo_path: &str) {
  let pr_cache = inner
    .cache
    .lock_or_recover()
    .get(repo_path)
    .cloned()
    .unwrap_or_default();

  // With the bulk call a full refresh costs one `gh` spawn, no more than the
  // CI rollups alone would.
  if pr_cache.is_empty() || inner.bulk_fetch.is_some() {
    poll_one_repo(inner, repo_path);
    return;
  }

  let mut next_ci: RepoCiCache = HashMap::new();
  for (branch, pr_info) in &pr_cache {
    fetch_ci_for_branch(inner, repo_path, branch, pr_info.is_some(), &mut next_ci);
  }

  {
    let mut ci_cache = inner.ci_cache.lock_or_recover();
    ci_cache.insert(repo_path.to_string(), next_ci.clone());
  }

  if let Some(cb) = inner.on_update.lock_or_recover().as_ref() {
    cb(repo_path, &pr_cache, &next_ci);
  }
}

/// Refresh PR info + CI for a single branch, merging into the existing repo
/// caches so sibling branches stay intact. Emits `on_update` with the full
/// repo snapshot afterward.
fn poll_one_branch(inner: &Inner, repo_path: &str, branch_name: &str) {
  let pr_info = match (inner.fetch)(repo_path, branch_name) {
    Ok(info) => info,
    Err(e) => {
      log::warn!("pr-status: branch fetch failed for {repo_path}#{branch_name}: {e}");
      // Keep prior entry on transient failure.
      if let Some(prev) = inner
        .cache
        .lock_or_recover()
        .get(repo_path)
        .and_then(|m| m.get(branch_name).cloned())
      {
        prev
      } else {
        return;
      }
    }
  };

  {
    let mut cache = inner.cache.lock_or_recover();
    cache
      .entry(repo_path.to_string())
      .or_default()
      .insert(branch_name.to_string(), pr_info.clone());
  }

  let mut next_ci_entry: Option<PrCiStatus> = None;
  let mut wrote_ci = false;
  if pr_info.is_some() {
    match (inner.ci_fetch)(repo_path, branch_name) {
      Ok(status) => {
        next_ci_entry = status;
        wrote_ci = true;
      }
      Err(e) => {
        log::warn!("pr-status: branch CI fetch failed for {repo_path}#{branch_name}: {e}");
        // Leave prior CI entry alone on failure.
      }
    }
  } else {
    next_ci_entry = None;
    wrote_ci = true;
  }

  if wrote_ci {
    let mut ci_cache = inner.ci_cache.lock_or_recover();
    ci_cache
      .entry(repo_path.to_string())
      .or_default()
      .insert(branch_name.to_string(), next_ci_entry);
  }

  let statuses = inner
    .cache
    .lock_or_recover()
    .get(repo_path)
    .cloned()
    .unwrap_or_default();
  let ci_statuses = inner
    .ci_cache
    .lock_or_recover()
    .get(repo_path)
    .cloned()
    .unwrap_or_default();

  if let Some(cb) = inner.on_update.lock_or_recover().as_ref() {
    cb(repo_path, &statuses, &ci_statuses);
  }
}

fn fetch_ci_for_branch(
  inner: &Inner,
  repo_path: &str,
  branch: &str,
  has_pr: bool,
  next_ci: &mut RepoCiCache,
) {
  // Only shell out to `gh pr checks` when a PR exists for this branch.
  if !has_pr {
    next_ci.insert(branch.to_string(), None);
    return;
  }

  match (inner.ci_fetch)(repo_path, branch) {
    Ok(status) => {
      next_ci.insert(branch.to_string(), status);
    }
    Err(e) => {
      log::warn!("pr-status: CI fetch failed for {repo_path}#{branch}: {e}");
      if let Some(prev) = inner
        .ci_cache
        .lock_or_recover()
        .get(repo_path)
        .and_then(|m| m.get(branch).cloned())
      {
        next_ci.insert(branch.to_string(), prev);
      }
    }
  }
}

// ── Production wiring ────────────────────────────────────────────────────────

static GLOBAL: std::sync::OnceLock<PrStatusManager> = std::sync::OnceLock::new();

fn production_fetch() -> PrFetchFn {
  Arc::new(|repo_path: &str, branch_name: &str| {
    let gh = crate::core::github_cli::gh_path()?;
    let path = crate::binary_paths::get_extended_path();
    crate::github::get_pr_info_via_gh_impl(&gh, repo_path, branch_name, &path)
  })
}

fn production_ci_fetch() -> CiFetchFn {
  Arc::new(|repo_path: &str, branch_name: &str| {
    let gh = crate::core::github_cli::gh_path()?;
    let path = crate::binary_paths::get_extended_path();
    crate::github::get_pr_checks_via_gh_impl(&gh, repo_path, branch_name, &path)
  })
}

/// One `gh pr list` per repo. Branches missing from a full page fall back to
/// `gh pr view`/`checks`, since their PR may predate the page.
fn production_bulk_fetch() -> BulkFetchFn {
  Arc::new(|repo_path: &str, branches: &[String]| {
    let gh = crate::core::github_cli::gh_path()?;
    let path = crate::binary_paths::get_extended_path();
    let mut listing = crate::github::list_prs_with_checks_via_gh_impl(&gh, repo_path, &path)?;
    let mut result = BulkFetchResult::new();
    for branch in branches {
      let entry = match listing.by_branch.remove(branch) {
        Some((pr, ci)) => (Some(pr), ci),
        None if listing.truncated => {
          let pr = crate::github::get_pr_info_via_gh_impl(&gh, repo_path, branch, &path)?;
          let ci = if pr.is_some() {
            crate::github::get_pr_checks_via_gh_impl(&gh, repo_path, branch, &path)?
          } else {
            None
          };
          (pr, ci)
        }
        None => (None, None),
      };
      result.insert(branch.clone(), entry);
    }
    Ok(result)
  })
}

fn production_list_branches() -> BranchListFn {
  Arc::new(|repo_path: &str| {
    let workspaces = crate::local_db::get_workspaces(repo_path)?;
    Ok(workspaces.into_iter().map(|w| w.branch_name).collect())
  })
}

/// Initialize (or return) the process-global manager.
pub fn global() -> &'static PrStatusManager {
  GLOBAL.get_or_init(|| {
    PrStatusManager::new_with_ci(
      production_fetch(),
      production_ci_fetch(),
      production_list_branches(),
    )
    .with_bulk_fetch(production_bulk_fetch())
  })
}

/// The map's entries in key order, so equal caches serialize identically.
fn statuses_sorted<V>(map: &HashMap<String, V>) -> std::collections::BTreeMap<&String, &V> {
  map.iter().collect()
}

/// What a poll found, as a number equal for equal caches, so the emitter
/// can tell whether anything changed since its last event.
pub fn statuses_digest(statuses: &RepoPrCache, ci_statuses: &RepoCiCache) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut hasher = std::collections::hash_map::DefaultHasher::new();
  serde_json::to_string(&(statuses_sorted(statuses), statuses_sorted(ci_statuses)))
    .unwrap_or_default()
    .hash(&mut hasher);
  hasher.finish()
}

/// The `pr-statuses-updated` payload. `stacks_moved` says a PR's base moved
/// (`core::pr_stacks`); `synced` lists the workspaces whose title or
/// description the PR just changed (`core::pr_sync`).
fn update_payload(repo_path: &str, stacks_moved: bool, synced: &[i64]) -> serde_json::Value {
  serde_json::json!({ "repo_path": repo_path, "stacks_moved": stacks_moved, "synced": synced })
}

/// The repository of a `pr-statuses-updated` payload whose workspaces the
/// PR changed, so windows read them again.
pub fn synced_repo(payload: &str) -> Option<String> {
  let payload: serde_json::Value = serde_json::from_str(payload).ok()?;
  let synced = payload["synced"].as_array()?;
  if synced.is_empty() {
    return None;
  }
  payload["repo_path"].as_str().map(str::to_string)
}

/// Attach a Tauri event emitter so cache refreshes notify the frontend.
pub fn set_app_handle(app: tauri::AppHandle) {
  use tauri::Emitter;
  // Listeners re-read the cache, so the event carries only which repo changed,
  // and only when what the poll found differs from the last event.
  let last_emitted: Mutex<HashMap<String, u64>> = Mutex::new(HashMap::new());
  global().set_on_update(Arc::new(move |repo_path, statuses, ci_statuses| {
    // Follow GitHub retargets and merged bases before the UI rereads stacks.
    let stacks_moved = crate::core::pr_stacks::sync_pr_bases(repo_path, statuses);
    // Before the event, so windows re-reading on it see the PR's text.
    let synced = crate::core::pr_sync::sync_repo(repo_path, statuses).unwrap_or_else(|err| {
      log::warn!("pr-status: syncing PR text into {repo_path}: {err}");
      Vec::new()
    });
    let digest = statuses_digest(statuses, ci_statuses);
    let unchanged = last_emitted
      .lock_or_recover()
      .insert(repo_path.to_string(), digest)
      == Some(digest);
    if unchanged && !stacks_moved && synced.is_empty() {
      return;
    }
    app
      .emit(
        "pr-statuses-updated",
        update_payload(repo_path, stacks_moved, &synced),
      )
      .ignore_closed();
  }));
  global().ensure_started();
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::AtomicUsize;

  fn sample_pr(number: u64, branch: &str) -> PrInfo {
    PrInfo {
      number,
      title: format!("PR {number}"),
      state: "OPEN".into(),
      url: format!("https://github.com/o/r/pull/{number}"),
      head_ref_name: branch.into(),
      base_ref_name: "main".into(),
      merge_state_status: Some("CLEAN".into()),
      is_draft: false,
      body: String::new(),
    }
  }

  fn sample_ci(state: &str) -> PrCiStatus {
    PrCiStatus {
      state: state.into(),
      total: 1,
      passed: if state == "success" { 1 } else { 0 },
      failed: if state == "failure" { 1 } else { 0 },
      pending: if state == "pending" { 1 } else { 0 },
      checks: vec![crate::github::PrCheckEntry {
        name: "build".into(),
        bucket: match state {
          "success" => "pass".into(),
          "failure" => "fail".into(),
          _ => "pending".into(),
        },
        link: "https://x/1".into(),
        duration_secs: None,
      }],
    }
  }

  fn manager_with(fetch: PrFetchFn, branches: Vec<String>) -> (PrStatusManager, Arc<AtomicUsize>) {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let fetch_count_clone = Arc::clone(&fetch_count);
    let wrapped: PrFetchFn = Arc::new(move |repo, branch| {
      fetch_count_clone.fetch_add(1, Ordering::SeqCst);
      fetch(repo, branch)
    });
    let list: BranchListFn = Arc::new(move |_| Ok(branches.clone()));
    (
      PrStatusManager::new(wrapped, list).with_interval(Duration::from_secs(60)),
      fetch_count,
    )
  }

  #[test]
  fn refresh_branch_now_fetches_pr_and_ci_for_one_branch() {
    let fetch: PrFetchFn = Arc::new(|_, branch| {
      if branch == "feat/open" {
        Ok(Some(sample_pr(11, branch)))
      } else {
        Ok(None)
      }
    });
    let ci_fetch: CiFetchFn = Arc::new(|_, branch| {
      assert_eq!(branch, "feat/open");
      Ok(Some(sample_ci("success")))
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat/open".into(), "feat/other".into()]));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list);

    mgr.refresh_branch_now("/tmp/repo", "feat/open");

    assert_eq!(
      mgr
        .get_cached("/tmp/repo", "feat/open")
        .unwrap()
        .unwrap()
        .number,
      11
    );
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat/open")
        .unwrap()
        .unwrap()
        .state,
      "success"
    );
    // Sibling branch left untouched (not yet polled).
    assert_eq!(mgr.get_cached("/tmp/repo", "feat/other"), None);
    assert_eq!(mgr.get_cached_ci("/tmp/repo", "feat/other"), None);
  }

  #[test]
  fn refresh_branch_now_skips_ci_when_branch_has_no_pr() {
    let fetch: PrFetchFn = Arc::new(|_, _| Ok(None));
    let ci_fetch: CiFetchFn = Arc::new(|_, _| {
      panic!("ci fetch must not run when there is no PR");
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list);

    mgr.refresh_branch_now("/tmp/repo", "feat");

    assert_eq!(mgr.get_cached("/tmp/repo", "feat"), Some(None));
    assert_eq!(mgr.get_cached_ci("/tmp/repo", "feat"), Some(None));
  }

  #[test]
  fn refresh_branch_now_emits_on_update_with_repo_caches() {
    let fetch: PrFetchFn = Arc::new(|_, b| Ok(Some(sample_pr(3, b))));
    let ci_fetch: CiFetchFn = Arc::new(|_, _| Ok(Some(sample_ci("pending"))));
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list);
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_cb = Arc::clone(&seen);
    mgr.set_on_update(Arc::new(move |repo, statuses, ci| {
      seen_cb
        .lock()
        .unwrap()
        .push(format!("{repo}:pr={}ci={}", statuses.len(), ci.len()));
    }));

    mgr.refresh_branch_now("/tmp/repo", "feat");
    assert_eq!(
      seen.lock_or_recover().clone(),
      vec!["/tmp/repo:pr=1ci=1".to_string()]
    );
  }

  #[test]
  fn queue_branch_refresh_updates_pr_and_ci_asynchronously() {
    let fetch: PrFetchFn = Arc::new(|_, b| Ok(Some(sample_pr(9, b))));
    let ci_fetch: CiFetchFn = Arc::new(|_, _| Ok(Some(sample_ci("success"))));
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list);

    mgr.queue_branch_refresh("/tmp/repo".into(), "feat".into());

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
      if mgr.get_cached("/tmp/repo", "feat").is_some()
        && mgr.get_cached_ci("/tmp/repo", "feat").is_some()
      {
        break;
      }
      thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
      mgr.get_cached("/tmp/repo", "feat").unwrap().unwrap().number,
      9
    );
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat")
        .unwrap()
        .unwrap()
        .state,
      "success"
    );
  }

  #[test]
  fn queue_branch_refresh_coalesces_inflight_duplicate() {
    let started = Arc::new(std::sync::Barrier::new(2));
    let started_fetch = Arc::clone(&started);
    let release = Arc::new(std::sync::Barrier::new(2));
    let release_fetch = Arc::clone(&release);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_fetch = Arc::clone(&calls);
    let fetch: PrFetchFn = Arc::new(move |_, b| {
      calls_fetch.fetch_add(1, Ordering::SeqCst);
      started_fetch.wait();
      release_fetch.wait();
      Ok(Some(sample_pr(1, b)))
    });
    let ci_fetch: CiFetchFn = Arc::new(|_, _| Ok(Some(sample_ci("pending"))));
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list);

    mgr.queue_branch_refresh("/tmp/repo".into(), "feat".into());
    started.wait();
    // Second queue while first is in-flight must not start another fetch.
    mgr.queue_branch_refresh("/tmp/repo".into(), "feat".into());
    release.wait();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
      if mgr.get_cached("/tmp/repo", "feat").is_some() {
        break;
      }
      thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(mgr.get_cached("/tmp/repo", "feat").unwrap().is_some());
  }

  #[test]
  fn refresh_ci_now_updates_ci_without_refetching_pr_info() {
    let pr_count = Arc::new(AtomicUsize::new(0));
    let pr_count_clone = Arc::clone(&pr_count);
    let fetch: PrFetchFn = Arc::new(move |_, branch| {
      pr_count_clone.fetch_add(1, Ordering::SeqCst);
      Ok(Some(sample_pr(1, branch)))
    });
    let ci_count = Arc::new(AtomicUsize::new(0));
    let ci_count_clone = Arc::clone(&ci_count);
    let ci_state = Arc::new(Mutex::new("pending".to_string()));
    let ci_state_clone = Arc::clone(&ci_state);
    let ci_fetch: CiFetchFn = Arc::new(move |_, _| {
      ci_count_clone.fetch_add(1, Ordering::SeqCst);
      Ok(Some(sample_ci(&ci_state_clone.lock_or_recover())))
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list)
      .with_intervals(Duration::from_secs(60), Duration::from_secs(30));

    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(pr_count.load(Ordering::SeqCst), 1);
    assert_eq!(ci_count.load(Ordering::SeqCst), 1);
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat")
        .unwrap()
        .unwrap()
        .state,
      "pending"
    );

    *ci_state.lock_or_recover() = "success".into();
    mgr.refresh_ci_now("/tmp/repo");
    assert_eq!(
      pr_count.load(Ordering::SeqCst),
      1,
      "CI-only refresh must not re-run gh pr view"
    );
    assert_eq!(ci_count.load(Ordering::SeqCst), 2);
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat")
        .unwrap()
        .unwrap()
        .state,
      "success"
    );
  }

  #[test]
  fn refresh_repo_now_caches_pr_info_per_branch() {
    let fetch: PrFetchFn = Arc::new(|_repo, branch| {
      if branch == "feat/a" {
        Ok(Some(sample_pr(1, branch)))
      } else {
        Ok(None)
      }
    });
    let (mgr, count) = manager_with(fetch, vec!["feat/a".into(), "feat/b".into()]);

    mgr.refresh_repo_now("/tmp/repo");

    let cached = mgr.list_cached("/tmp/repo");
    assert_eq!(cached.len(), 2);
    assert_eq!(cached.get("feat/a").unwrap().as_ref().unwrap().number, 1);
    assert!(cached.get("feat/b").unwrap().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 2);
  }

  #[test]
  fn refresh_repo_now_caches_ci_status_only_for_branches_with_prs() {
    let fetch: PrFetchFn = Arc::new(|_repo, branch| {
      if branch == "feat/a" {
        Ok(Some(sample_pr(1, branch)))
      } else {
        Ok(None)
      }
    });
    let ci_fetch: CiFetchFn = Arc::new(|_repo, branch| {
      if branch == "feat/a" {
        Ok(Some(sample_ci("success")))
      } else {
        // Must not be called for branches without a PR.
        panic!("ci fetch should not run for branch without PR: {branch}");
      }
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat/a".into(), "feat/b".into()]));
    let mgr =
      PrStatusManager::new_with_ci(fetch, ci_fetch, list).with_interval(Duration::from_secs(60));

    mgr.refresh_repo_now("/tmp/repo");

    let ci = mgr.list_cached_ci("/tmp/repo");
    assert_eq!(ci.len(), 2);
    assert_eq!(ci.get("feat/a").unwrap().as_ref().unwrap().state, "success");
    assert!(ci.get("feat/b").unwrap().is_none());
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat/a")
        .unwrap()
        .unwrap()
        .passed,
      1
    );
    assert_eq!(mgr.get_cached_ci("/tmp/repo", "feat/b"), Some(None));
    assert_eq!(mgr.get_cached_ci("/tmp/repo", "never-listed"), None);
  }

  #[test]
  fn list_cached_ci_is_read_only_and_does_not_refetch() {
    let fetch: PrFetchFn = Arc::new(|_, branch| Ok(Some(sample_pr(7, branch))));
    let ci_count = Arc::new(AtomicUsize::new(0));
    let ci_count_clone = Arc::clone(&ci_count);
    let ci_fetch: CiFetchFn = Arc::new(move |_, _| {
      ci_count_clone.fetch_add(1, Ordering::SeqCst);
      Ok(Some(sample_ci("pending")))
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr =
      PrStatusManager::new_with_ci(fetch, ci_fetch, list).with_interval(Duration::from_secs(60));

    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(ci_count.load(Ordering::SeqCst), 1);

    let _ = mgr.list_cached_ci("/tmp/repo");
    let _ = mgr.get_cached_ci("/tmp/repo", "feat");
    assert_eq!(ci_count.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn ci_fetch_error_preserves_previous_cache_entry() {
    let fail = Arc::new(AtomicBool::new(false));
    let fail_flag = Arc::clone(&fail);
    let fetch: PrFetchFn = Arc::new(|_, branch| Ok(Some(sample_pr(9, branch))));
    let ci_fetch: CiFetchFn = Arc::new(move |_, _| {
      if fail_flag.load(Ordering::SeqCst) {
        Err("gh checks failed".into())
      } else {
        Ok(Some(sample_ci("success")))
      }
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr =
      PrStatusManager::new_with_ci(fetch, ci_fetch, list).with_interval(Duration::from_secs(60));

    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(
      mgr
        .list_cached_ci("/tmp/repo")
        .get("feat")
        .unwrap()
        .as_ref()
        .unwrap()
        .state,
      "success"
    );

    fail.store(true, Ordering::SeqCst);
    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(
      mgr
        .list_cached_ci("/tmp/repo")
        .get("feat")
        .unwrap()
        .as_ref()
        .unwrap()
        .state,
      "success"
    );
  }

  #[test]
  fn list_cached_is_read_only_and_does_not_refetch() {
    let fetch: PrFetchFn = Arc::new(|_, branch| Ok(Some(sample_pr(7, branch))));
    let (mgr, count) = manager_with(fetch, vec!["feat".into()]);

    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(count.load(Ordering::SeqCst), 1);

    let _ = mgr.list_cached("/tmp/repo");
    let _ = mgr.list_cached("/tmp/repo");
    assert_eq!(count.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn statuses_digest_ignores_order_and_sees_any_field_change() {
    let mut a = RepoPrCache::new();
    let mut b = RepoPrCache::new();
    for branch in ["x", "y", "z"] {
      a.insert(branch.into(), Some(sample_pr(1, branch)));
    }
    for branch in ["z", "y", "x"] {
      b.insert(branch.into(), Some(sample_pr(1, branch)));
    }
    let ci = RepoCiCache::from([("x".to_string(), Some(sample_ci("success")))]);
    assert_eq!(statuses_digest(&a, &ci), statuses_digest(&b, &ci));

    b.get_mut("y").unwrap().as_mut().unwrap().is_draft = true;
    assert_ne!(statuses_digest(&a, &ci), statuses_digest(&b, &ci));
    let failing = RepoCiCache::from([("x".to_string(), Some(sample_ci("failure")))]);
    assert_ne!(statuses_digest(&a, &ci), statuses_digest(&a, &failing));
  }

  #[test]
  fn get_cached_distinguishes_missing_vs_no_pr() {
    let fetch: PrFetchFn = Arc::new(|_, branch| {
      if branch == "has-pr" {
        Ok(Some(sample_pr(3, branch)))
      } else {
        Ok(None)
      }
    });
    let (mgr, _) = manager_with(fetch, vec!["has-pr".into(), "no-pr".into()]);

    assert_eq!(mgr.get_cached("/tmp/repo", "has-pr"), None);

    mgr.refresh_repo_now("/tmp/repo");

    match mgr.get_cached("/tmp/repo", "has-pr") {
      Some(Some(info)) => assert_eq!(info.number, 3),
      other => panic!("expected Some(Some(pr)), got {other:?}"),
    }
    assert_eq!(mgr.get_cached("/tmp/repo", "no-pr"), Some(None));
    assert_eq!(mgr.get_cached("/tmp/repo", "never-listed"), None);
  }

  #[test]
  fn fetch_error_preserves_previous_cache_entry() {
    let fail = Arc::new(AtomicBool::new(false));
    let fail_flag = Arc::clone(&fail);
    let fetch: PrFetchFn = Arc::new(move |_, branch| {
      if fail_flag.load(Ordering::SeqCst) {
        Err("gh auth failed".into())
      } else {
        Ok(Some(sample_pr(9, branch)))
      }
    });
    let (mgr, _) = manager_with(fetch, vec!["feat".into()]);

    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(
      mgr
        .list_cached("/tmp/repo")
        .get("feat")
        .unwrap()
        .as_ref()
        .unwrap()
        .number,
      9
    );

    fail.store(true, Ordering::SeqCst);
    mgr.refresh_repo_now("/tmp/repo");
    assert_eq!(
      mgr
        .list_cached("/tmp/repo")
        .get("feat")
        .unwrap()
        .as_ref()
        .unwrap()
        .number,
      9
    );
  }

  #[test]
  fn watch_repo_registers_and_unwatch_clears() {
    let fetch: PrFetchFn = Arc::new(|_, _| Ok(None));
    let (mgr, _) = manager_with(fetch, vec!["feat".into()]);

    assert!(!mgr.is_watching("/tmp/repo"));
    mgr.watch_repo("/tmp/repo");
    assert!(mgr.is_watching("/tmp/repo"));
    mgr.unwatch_repo("/tmp/repo");
    assert!(!mgr.is_watching("/tmp/repo"));
    mgr.shutdown();
  }

  #[test]
  fn retain_watched_drops_repos_no_window_shows() {
    let fetch: PrFetchFn = Arc::new(|_, _| Ok(None));
    let (mgr, _) = manager_with(fetch, vec!["feat".into()]);

    mgr.watch_repo("/tmp/shown");
    mgr.watch_repo("/tmp/closed");
    mgr.retain_watched(|repo| repo == "/tmp/shown");
    assert!(mgr.is_watching("/tmp/shown"));
    assert!(!mgr.is_watching("/tmp/closed"));
    mgr.shutdown();
  }

  #[test]
  fn rewatching_a_repo_does_not_force_a_poll() {
    use std::sync::atomic::AtomicUsize;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_fetch = Arc::clone(&calls);
    let fetch: PrFetchFn = Arc::new(move |_, _| {
      calls_fetch.fetch_add(1, Ordering::SeqCst);
      Ok(None)
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let hour = Duration::from_secs(3600);
    let mgr = PrStatusManager::new(fetch, list).with_intervals(hour, hour);

    mgr.watch_repo("/tmp/rewatch");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while calls.load(Ordering::SeqCst) == 0 {
      assert!(std::time::Instant::now() < deadline, "first poll never ran");
      thread::sleep(Duration::from_millis(10));
    }

    // GPUI reads re-watch on every render.
    for _ in 0..50 {
      mgr.watch_repo("/tmp/rewatch");
    }
    thread::sleep(Duration::from_millis(200));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    mgr.shutdown();
  }

  #[test]
  fn polls_slow_down_in_the_background_and_catch_up_in_front() {
    use std::sync::atomic::AtomicUsize;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_fetch = Arc::clone(&calls);
    let fetch: PrFetchFn = Arc::new(move |_, _| {
      calls_fetch.fetch_add(1, Ordering::SeqCst);
      Ok(None)
    });
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let every = Duration::from_millis(100);
    let mgr = PrStatusManager::new(fetch, list).with_intervals(every, every);
    mgr.set_background(true);

    mgr.watch_repo("/tmp/background");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while calls.load(Ordering::SeqCst) == 0 {
      assert!(std::time::Instant::now() < deadline, "first poll never ran");
      thread::sleep(Duration::from_millis(10));
    }
    // Several foreground intervals, well inside one background interval.
    thread::sleep(every * 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    mgr.set_background(false);
    let deadline = std::time::Instant::now() + every;
    while calls.load(Ordering::SeqCst) == 1 {
      assert!(
        std::time::Instant::now() < deadline,
        "coming to the front polls what is due"
      );
      thread::sleep(Duration::from_millis(5));
    }
    mgr.shutdown();
  }

  #[test]
  fn synced_repo_names_repo_when_workspaces_changed() {
    let payload = update_payload("/r", false, &[3]);
    assert_eq!(synced_repo(&payload.to_string()).as_deref(), Some("/r"));
  }

  #[test]
  fn synced_repo_is_none_when_nothing_synced() {
    let payload = update_payload("/r", false, &[]);
    assert_eq!(synced_repo(&payload.to_string()), None);
  }

  #[test]
  fn on_update_fires_after_refresh() {
    let fetch: PrFetchFn = Arc::new(|_, b| Ok(Some(sample_pr(1, b))));
    let (mgr, _) = manager_with(fetch, vec!["feat".into()]);
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_cb = Arc::clone(&seen);
    mgr.set_on_update(Arc::new(move |repo, statuses, _ci| {
      seen_cb
        .lock()
        .unwrap()
        .push(format!("{repo}:{}", statuses.len()));
    }));

    mgr.refresh_repo_now("/tmp/repo");
    let events = seen.lock_or_recover().clone();
    assert_eq!(events, vec!["/tmp/repo:1".to_string()]);
  }

  #[test]
  fn background_loop_polls_watched_repo() {
    let fetch: PrFetchFn = Arc::new(|_, b| Ok(Some(sample_pr(42, b))));
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let mgr = PrStatusManager::new(fetch, list).with_interval(Duration::from_millis(50));

    mgr.watch_repo("/tmp/bg");
    // watch_repo wakes the loop; wait until cache is populated.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
      if mgr
        .list_cached("/tmp/bg")
        .get("feat")
        .and_then(|v| v.as_ref())
        .is_some()
      {
        break;
      }
      if std::time::Instant::now() > deadline {
        panic!("background poll did not populate cache");
      }
      thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(
      mgr
        .list_cached("/tmp/bg")
        .get("feat")
        .unwrap()
        .as_ref()
        .unwrap()
        .number,
      42
    );
    mgr.shutdown();
  }

  #[test]
  fn bulk_fetch_replaces_per_branch_fetches() {
    let fetch: PrFetchFn = Arc::new(|_, _| panic!("per-branch fetch must not run"));
    let ci_fetch: CiFetchFn = Arc::new(|_, _| panic!("per-branch CI fetch must not run"));
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat/open".into(), "feat/none".into()]));
    let bulk: BulkFetchFn = Arc::new(|_, branches| {
      Ok(
        branches
          .iter()
          .map(|b| {
            let entry = if b == "feat/open" {
              (Some(sample_pr(5, b)), Some(sample_ci("success")))
            } else {
              (None, None)
            };
            (b.clone(), entry)
          })
          .collect(),
      )
    });
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list).with_bulk_fetch(bulk);

    poll_one_repo(&mgr.inner, "/tmp/repo");

    assert_eq!(
      mgr
        .get_cached("/tmp/repo", "feat/open")
        .unwrap()
        .unwrap()
        .number,
      5
    );
    assert_eq!(mgr.get_cached("/tmp/repo", "feat/none"), Some(None));
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat/open")
        .unwrap()
        .unwrap()
        .state,
      "success"
    );
  }

  #[test]
  fn failed_bulk_fetch_falls_back_to_per_branch_fetches() {
    let fetch: PrFetchFn = Arc::new(|_, b| Ok(Some(sample_pr(8, b))));
    let ci_fetch: CiFetchFn = Arc::new(|_, _| Ok(Some(sample_ci("pending"))));
    let list: BranchListFn = Arc::new(|_| Ok(vec!["feat".into()]));
    let bulk: BulkFetchFn = Arc::new(|_, _| Err("gh pr list failed".into()));
    let mgr = PrStatusManager::new_with_ci(fetch, ci_fetch, list).with_bulk_fetch(bulk);

    poll_one_repo(&mgr.inner, "/tmp/repo");

    assert_eq!(
      mgr.get_cached("/tmp/repo", "feat").unwrap().unwrap().number,
      8
    );
    assert_eq!(
      mgr
        .get_cached_ci("/tmp/repo", "feat")
        .unwrap()
        .unwrap()
        .state,
      "pending"
    );
  }
}
