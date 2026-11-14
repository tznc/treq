//! Filtering the GitHub lists the desktop and mobile apps show: the search
//! text and the Filters selects, over the items already loaded.

use std::collections::{HashMap, HashSet};

use crate::github::{GhIssue, GhPullRequest};

/// `PrFilters`: every set field must match.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrFilters {
  /// Free text matched against title, number, branches and author.
  pub query: String,
  pub author: Option<String>,
  pub head: Option<String>,
  pub base: Option<String>,
  pub stack_level: Option<u32>,
  pub opened_within_days: Option<i64>,
  /// "conflicting" or "clean".
  pub conflict: Option<String>,
  pub label: Option<String>,
}

impl PrFilters {
  /// `activePrFilterCount`: set filters, without the search text.
  pub fn active_count(&self) -> usize {
    [
      self.author.is_some(),
      self.head.is_some(),
      self.base.is_some(),
      self.stack_level.is_some(),
      self.opened_within_days.is_some(),
      self.conflict.is_some(),
      self.label.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count()
  }

  /// "Clear filters": everything but the search text.
  pub fn cleared(&self) -> Self {
    Self {
      query: self.query.clone(),
      ..Self::default()
    }
  }
}

/// `prStackLevels`: 1 at the bottom, +1 for each listed PR below it.
pub fn stack_levels(prs: &[GhPullRequest]) -> HashMap<u64, u32> {
  let by_head: HashMap<&str, &GhPullRequest> = prs
    .iter()
    // A fork PR from its own `main` into `main` is no parent of every PR into `main`.
    .filter(|pr| pr.head_ref_name != pr.base_ref_name)
    .map(|pr| (pr.head_ref_name.as_str(), pr))
    .collect();
  prs
    .iter()
    .map(|pr| {
      let mut seen = HashSet::new();
      let mut level = 1;
      let mut parent = by_head.get(pr.base_ref_name.as_str());
      while let Some(p) = parent {
        if p.number == pr.number || !seen.insert(p.number) {
          break;
        }
        level += 1;
        parent = by_head.get(p.base_ref_name.as_str());
      }
      (pr.number, level)
    })
    .collect()
}

/// Whether the PR matches `q`, the trimmed, lowercased search text.
fn matches_query(pr: &GhPullRequest, q: &str) -> bool {
  if q.is_empty() {
    return true;
  }
  let has = |field: &str| {
    // ASCII text, the common case, is compared without allocating.
    if field.is_ascii() {
      field.len() >= q.len()
        && field
          .as_bytes()
          .windows(q.len())
          .any(|w| w.eq_ignore_ascii_case(q.as_bytes()))
    } else {
      field.to_lowercase().contains(q)
    }
  };
  has(&pr.title)
    || has(&pr.head_ref_name)
    || has(&pr.base_ref_name)
    || has(&pr.author.login)
    || format!("#{}", pr.number).contains(q)
}

fn matches_conflict(pr: &GhPullRequest, conflict: Option<&str>) -> bool {
  let status = pr.merge_state_status.as_deref().map(str::to_uppercase);
  match conflict {
    None => true,
    Some("conflicting") => status.as_deref() == Some("DIRTY"),
    // An unknown status is no proof of "no conflicts".
    Some(_) => status.is_some_and(|s| !s.is_empty() && s != "DIRTY" && s != "UNKNOWN"),
  }
}

/// `filterPrs`, as of `now`.
#[cfg(test)]
pub fn filter_prs<'a>(
  prs: &'a [GhPullRequest],
  filters: &PrFilters,
  now: chrono::DateTime<chrono::Utc>,
) -> Vec<&'a GhPullRequest> {
  filter_pr_indices(prs, filters, now)
    .into_iter()
    .map(|ix| &prs[ix])
    .collect()
}

/// `filterPrs`, as the indices of the PRs it keeps.
pub fn filter_pr_indices(
  prs: &[GhPullRequest],
  filters: &PrFilters,
  now: chrono::DateTime<chrono::Utc>,
) -> Vec<usize> {
  let query = filters.query.trim().to_lowercase();
  let levels = filters
    .stack_level
    .map(|_| stack_levels(prs))
    .unwrap_or_default();
  let opened = |pr: &GhPullRequest| match filters.opened_within_days {
    None => true,
    Some(days) => chrono::DateTime::parse_from_rfc3339(&pr.created_at)
      .is_ok_and(|at| now.signed_duration_since(at) <= chrono::Duration::days(days)),
  };
  let is = |value: &Option<String>, actual: &str| value.as_deref().is_none_or(|v| v == actual);
  prs
    .iter()
    .enumerate()
    .filter(|(_, pr)| {
      matches_query(pr, &query)
        && is(&filters.author, &pr.author.login)
        && is(&filters.head, &pr.head_ref_name)
        && is(&filters.base, &pr.base_ref_name)
        && filters
          .stack_level
          .is_none_or(|level| levels.get(&pr.number) == Some(&level))
        && opened(pr)
        && matches_conflict(pr, filters.conflict.as_deref())
        && filters
          .label
          .as_deref()
          .is_none_or(|label| pr.labels.iter().any(|l| l.name == label))
    })
    .map(|(ix, _)| ix)
    .collect()
}

/// `prFilterOptions`: the distinct values of the loaded PRs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilterOptions {
  pub authors: Vec<String>,
  pub heads: Vec<String>,
  pub bases: Vec<String>,
  pub labels: Vec<String>,
  pub stack_levels: Vec<u32>,
}

fn sorted<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
  let mut values: Vec<String> = values
    .collect::<HashSet<_>>()
    .into_iter()
    .map(str::to_string)
    .collect();
  // `localeCompare`: case-insensitive first.
  values.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
  values
}

pub fn filter_options(prs: &[GhPullRequest]) -> FilterOptions {
  let mut levels: Vec<u32> = stack_levels(prs)
    .into_values()
    .collect::<HashSet<_>>()
    .into_iter()
    .collect();
  levels.sort_unstable();
  FilterOptions {
    authors: sorted(prs.iter().map(|pr| pr.author.login.as_str())),
    heads: sorted(prs.iter().map(|pr| pr.head_ref_name.as_str())),
    bases: sorted(prs.iter().map(|pr| pr.base_ref_name.as_str())),
    labels: sorted(
      prs
        .iter()
        .flat_map(|pr| pr.labels.iter().map(|l| l.name.as_str())),
    ),
    stack_levels: levels,
  }
}

/// The Issues list's filters: every set field must match.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IssueFilters {
  /// Free text matched against the title and number.
  pub query: String,
  pub author: Option<String>,
  pub label: Option<String>,
}

impl IssueFilters {
  /// Set filters, without the search text.
  pub fn active_count(&self) -> usize {
    usize::from(self.author.is_some()) + usize::from(self.label.is_some())
  }

  /// "Clear filters": everything but the search text.
  pub fn cleared(&self) -> Self {
    Self {
      query: self.query.clone(),
      ..Self::default()
    }
  }
}

/// The indices of the issues `filters` keeps.
pub fn filter_issue_indices(issues: &[GhIssue], filters: &IssueFilters) -> Vec<usize> {
  let query = filters.query.trim().to_lowercase();
  issues
    .iter()
    .enumerate()
    .filter(|(_, issue)| {
      (query.is_empty()
        || issue.title.to_lowercase().contains(&query)
        || format!("#{}", issue.number).contains(&query))
        && filters
          .author
          .as_deref()
          .is_none_or(|a| a == issue.author.login)
        && filters
          .label
          .as_deref()
          .is_none_or(|label| issue.labels.iter().any(|l| l.name == label))
    })
    .map(|(ix, _)| ix)
    .collect()
}

/// What the Issues list's Filters offer: the loaded issues' values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IssueFilterOptions {
  pub authors: Vec<String>,
  pub labels: Vec<String>,
}

pub fn issue_filter_options(issues: &[GhIssue]) -> IssueFilterOptions {
  IssueFilterOptions {
    authors: sorted(issues.iter().map(|i| i.author.login.as_str())),
    labels: sorted(
      issues
        .iter()
        .flat_map(|i| i.labels.iter().map(|l| l.name.as_str())),
    ),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::{GhAuthor, GhLabel};

  fn pr(number: u64, head: &str, base: &str) -> GhPullRequest {
    GhPullRequest {
      number,
      title: format!("PR {number}"),
      state: "OPEN".into(),
      url: format!("https://github.com/acme/treq/pull/{number}"),
      body: None,
      author: GhAuthor {
        login: "zn".into(),
        avatar_url: None,
      },
      labels: Vec::new(),
      head_ref_name: head.into(),
      base_ref_name: base.into(),
      merge_state_status: None,
      created_at: "2026-10-19T00:00:00Z".into(),
      updated_at: "2026-10-19T00:00:00Z".into(),
      comments: None,
      is_draft: false,
      review_decision: None,
      latest_reviews: vec![],
      reviews: vec![],
    }
  }
  fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T00:00:00Z".parse().expect("now")
  }

  fn numbers(prs: Vec<&GhPullRequest>) -> Vec<u64> {
    prs.into_iter().map(|p| p.number).collect()
  }

  #[test]
  fn stacks_count_the_listed_prs_below() {
    let prs = [
      pr(1, "a", "main"),
      pr(2, "b", "a"),
      pr(3, "c", "b"),
      pr(4, "main", "main"),
    ];
    let levels = stack_levels(&prs);
    assert_eq!(levels[&1], 1);
    assert_eq!(levels[&2], 2);
    assert_eq!(levels[&3], 3);
    assert_eq!(levels[&4], 1);
    // A cycle ends instead of looping.
    let cycle = [pr(1, "a", "b"), pr(2, "b", "a")];
    assert_eq!(stack_levels(&cycle)[&1], 2);
  }

  #[test]
  fn filters_match_every_set_field() {
    let mut fixed = pr(2, "fix/login", "main");
    fixed.author.login = "ada".into();
    fixed.labels = vec![GhLabel {
      name: "bug".into(),
      color: "ff0000".into(),
    }];
    fixed.merge_state_status = Some("dirty".into());
    let mut old = pr(3, "feat/x", "fix/login");
    old.created_at = "2026-09-01T00:00:00Z".into();
    old.merge_state_status = Some("CLEAN".into());
    let prs = [pr(1, "a", "main"), fixed, old];

    let by = |f: PrFilters| numbers(filter_prs(&prs, &f, now()));
    assert_eq!(by(PrFilters::default()), [1, 2, 3]);
    let query = |q: &str| PrFilters {
      query: q.into(),
      ..PrFilters::default()
    };
    assert_eq!(by(query(" LOGIN ")), [2, 3]);
    assert_eq!(by(query("#3")), [3]);
    assert_eq!(by(query("ada")), [2]);
    assert_eq!(
      by(PrFilters {
        label: Some("bug".into()),
        ..PrFilters::default()
      }),
      [2]
    );
    assert_eq!(
      by(PrFilters {
        conflict: Some("conflicting".into()),
        ..PrFilters::default()
      }),
      [2]
    );
    // No status is no proof of no conflicts.
    assert_eq!(
      by(PrFilters {
        conflict: Some("clean".into()),
        ..PrFilters::default()
      }),
      [3]
    );
    assert_eq!(
      by(PrFilters {
        opened_within_days: Some(7),
        ..PrFilters::default()
      }),
      [1, 2]
    );
    assert_eq!(
      by(PrFilters {
        stack_level: Some(2),
        base: Some("fix/login".into()),
        ..PrFilters::default()
      }),
      [3]
    );
  }

  #[test]
  fn counts_and_clears_filters_but_not_the_search() {
    let filters = PrFilters {
      query: "x".into(),
      author: Some("a".into()),
      stack_level: Some(1),
      ..PrFilters::default()
    };
    assert_eq!(filters.active_count(), 2);
    assert_eq!(
      filters.cleared(),
      PrFilters {
        query: "x".into(),
        ..PrFilters::default()
      }
    );
  }

  #[test]
  fn options_are_distinct_and_sorted() {
    let mut b = pr(2, "Zeta", "main");
    b.author.login = "bob".into();
    let options = filter_options(&[pr(1, "alpha", "main"), b, pr(3, "beta", "alpha")]);
    assert_eq!(options.authors, ["bob", "zn"]);
    assert_eq!(options.heads, ["alpha", "beta", "Zeta"]);
    assert_eq!(options.bases, ["alpha", "main"]);
    assert_eq!(options.stack_levels, [1, 2]);
  }

  fn issue(number: u64, title: &str, who: &str, labels: &[&str]) -> crate::github::GhIssue {
    crate::github::GhIssue {
      number,
      title: title.into(),
      state: "OPEN".into(),
      url: String::new(),
      body: None,
      author: GhAuthor {
        login: who.into(),
        avatar_url: None,
      },
      labels: labels
        .iter()
        .map(|l| GhLabel {
          name: (*l).into(),
          color: String::new(),
        })
        .collect(),
      created_at: String::new(),
      updated_at: String::new(),
      comments: None,
    }
  }

  #[test]
  fn issue_filters_match_search_author_and_label() {
    let issues = [
      issue(1, "Crash on launch", "ada", &["bug"]),
      issue(2, "Dark mode", "bob", &["enhancement"]),
      issue(3, "Crash in terminal", "bob", &["bug", "mobile"]),
    ];
    let by = |f: IssueFilters| -> Vec<u64> {
      filter_issue_indices(&issues, &f)
        .into_iter()
        .map(|i| issues[i].number)
        .collect()
    };
    assert_eq!(by(IssueFilters::default()), [1, 2, 3]);
    let f = |query: &str, author: Option<&str>, label: Option<&str>| IssueFilters {
      query: query.into(),
      author: author.map(Into::into),
      label: label.map(Into::into),
    };
    assert_eq!(by(f(" CRASH ", None, None)), [1, 3]);
    assert_eq!(by(f("#2", None, None)), [2]);
    assert_eq!(by(f("", Some("bob"), None)), [2, 3]);
    assert_eq!(by(f("", Some("bob"), Some("bug"))), [3]);
    assert_eq!(f("x", Some("bob"), Some("bug")).active_count(), 2);
    assert_eq!(f("x", Some("bob"), None).cleared(), f("x", None, None));
  }

  #[test]
  fn issue_options_are_distinct_and_sorted() {
    let options = issue_filter_options(&[
      issue(1, "a", "zed", &["bug"]),
      issue(2, "b", "Ada", &["Docs", "bug"]),
    ]);
    assert_eq!(options.authors, ["Ada", "zed"]);
    assert_eq!(options.labels, ["bug", "Docs"]);
  }
}
