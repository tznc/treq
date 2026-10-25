//! A repository's GitHub page: Issues and Pull requests as two tabs over
//! the same lists their own pages show, like the desktop app's GitHub page.
//! It opens on Issues, as desktop does.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, Sizable,
  },
  *,
};

use super::{issues::IssuesScreen, pull_requests::PullRequestsScreen, workspaces::repo_name};
use crate::ui::{nav, sheet, Location};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
  #[default]
  Issues,
  PullRequests,
}

impl Tab {
  pub const ALL: [Tab; 2] = [Tab::Issues, Tab::PullRequests];

  pub fn label(self) -> &'static str {
    match self {
      Tab::Issues => "Issues",
      Tab::PullRequests => "Pull requests",
    }
  }

  fn id(self) -> &'static str {
    match self {
      Tab::Issues => "github-tab-issues",
      Tab::PullRequests => "github-tab-prs",
    }
  }
}

/// Opens the GitHub page of the repository at `repo` on the host.
pub fn open(repo: String, cx: &mut App) {
  let screen: Rc<RefCell<Option<WeakEntity<GithubScreen>>>> = Default::default();
  let menu_screen = screen.clone();
  nav::push_with(
    format!("GitHub · {}", repo_name(&repo)),
    nav::Header {
      on_title: None,
      more: Some(Rc::new(move |_, cx| {
        if let Some(screen) = menu_screen.borrow().as_ref() {
          screen.update(cx, |screen, cx| screen.menu(cx)).ok();
        }
      })),
    },
    Location {
      repo: Some(repo.clone()),
      workspace: None,
    },
    cx,
    move |window, cx| {
      let entity = cx.new(|cx| GithubScreen::new(repo, window, cx));
      *screen.borrow_mut() = Some(entity.downgrade());
      entity
    },
  );
}

pub struct GithubScreen {
  repo: String,
  tab: Tab,
  issues: Entity<IssuesScreen>,
  /// Made on first switch, so opening the page reads only the issues.
  prs: Option<Entity<PullRequestsScreen>>,
}

impl GithubScreen {
  fn new(repo: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let issues = cx.new(|cx| IssuesScreen::new(repo.clone(), window, cx));
    Self {
      repo,
      tab: Tab::default(),
      issues,
      prs: None,
    }
  }

  fn select(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
    self.tab = tab;
    if tab == Tab::PullRequests && self.prs.is_none() {
      let repo = self.repo.clone();
      self.prs = Some(cx.new(|cx| PullRequestsScreen::new(repo, window, cx)));
    }
    cx.notify();
  }

  /// The title bar's ⋯: the open tab's menu.
  fn menu(&mut self, cx: &mut Context<Self>) {
    match (self.tab, &self.prs) {
      (Tab::PullRequests, Some(prs)) => {
        let prs = prs.downgrade();
        sheet::menu(
          "Pull requests",
          vec![sheet::MenuEntry::new(
            "menu-refresh",
            "Refresh",
            move |_, cx| {
              prs.update(cx, |screen, cx| screen.reload(cx)).ok();
            },
          )],
          cx,
        );
      }
      _ => self.issues.update(cx, |screen, cx| screen.menu(cx)),
    }
  }
}

impl Render for GithubScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let tabs = h_flex().px_4().pt_3().gap_2().children(Tab::ALL.map(|tab| {
      Button::new(tab.id())
        .large()
        .flex_1()
        .h(px(44.))
        .label(tab.label())
        .map(|b| {
          if tab == self.tab {
            b.primary()
          } else {
            b.outline()
          }
        })
        .on_click(cx.listener(move |this, _, window, cx| this.select(tab, window, cx)))
    }));
    let body: AnyView = match (self.tab, &self.prs) {
      (Tab::PullRequests, Some(prs)) => prs.clone().into(),
      _ => self.issues.clone().into(),
    };
    v_flex()
      .size_full()
      .child(tabs)
      .child(div().flex_1().min_h_0().child(body))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[core::prelude::v1::test]
  fn opens_on_issues_with_issues_first() {
    assert_eq!(Tab::default(), Tab::Issues);
    assert_eq!(Tab::ALL.map(Tab::label), ["Issues", "Pull requests"]);
  }
}
