//! Back and forward through the places the window showed, as a browser's
//! history does: the mouse's back and forward buttons walk it.

use super::model::{Model, Page, Tab};

/// How many places the window remembers.
const KEPT: usize = 100;

/// A place the window showed: its page, repository, workspace and tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
  page: Page,
  supporting: Option<String>,
  selected: Option<i64>,
  tab: Tab,
}

impl Location {
  fn of(model: &Model) -> Self {
    let mut page = model.page.clone();
    // A reload reads the same pull request again; it is not a new place.
    if let Page::Github(github) = &mut page {
      github.reload = 0;
    }
    Self {
      page,
      supporting: model.supporting.clone(),
      selected: model.selected,
      tab: model.view.tab,
    }
  }
}

/// The places shown, oldest first, and which one is on screen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct History {
  entries: Vec<Location>,
  index: usize,
}

impl History {
  pub fn can_go(&self, back: bool) -> bool {
    match back {
      true => self.index > 0,
      false => self.index + 1 < self.entries.len(),
    }
  }
}

impl Model {
  /// Remembers the place on screen, unless it is the one last remembered.
  /// A new place drops the ones ahead of it, as a browser does.
  pub fn record_history(&mut self) {
    let here = Location::of(self);
    let history = &mut self.history;
    if history.entries.get(history.index) == Some(&here) {
      return;
    }
    if !history.entries.is_empty() {
      history.entries.truncate(history.index + 1);
    }
    history.entries.push(here);
    if history.entries.len() > KEPT {
      history.entries.remove(0);
    }
    history.index = history.entries.len() - 1;
  }

  /// Shows the place before (`back`) or after the one on screen.
  pub fn go(&mut self, back: bool) {
    if !self.history.can_go(back) {
      return;
    }
    match back {
      true => self.history.index -= 1,
      false => self.history.index += 1,
    }
    let Location {
      page,
      supporting,
      selected,
      tab,
    } = self.history.entries[self.history.index].clone();
    if self.page != page {
      self.back = std::mem::replace(&mut self.page, page);
    }
    self.supporting = supporting;
    self.set_selected(selected);
    self.view.tab = tab;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn model() -> Model {
    let mut model = Model::new(Some("/repo".into()));
    model.record_history();
    model
  }

  fn visit(model: &mut Model, change: impl FnOnce(&mut Model)) {
    change(model);
    model.record_history();
  }

  #[test]
  fn back_and_forward_walk_the_pages_shown() {
    let mut model = model();
    visit(&mut model, Model::open_github);
    visit(&mut model, Model::open_linear);
    model.go(true);
    assert!(matches!(model.page, Page::Github(_)));
    model.go(true);
    assert_eq!(model.page, Page::Workspace);
    model.go(true);
    assert_eq!(model.page, Page::Workspace);
    model.go(false);
    model.go(false);
    assert_eq!(model.page, Page::Linear);
    model.go(false);
    assert_eq!(model.page, Page::Linear);
  }

  #[test]
  fn going_back_restores_the_workspace_and_its_tab() {
    let mut model = model();
    visit(&mut model, |m| m.set_selected(Some(7)));
    visit(&mut model, |m| m.view.tab = Tab::Changes);
    visit(&mut model, |m| m.set_selected(Some(9)));
    model.go(true);
    assert_eq!((model.selected, model.view.tab), (Some(7), Tab::Changes));
  }

  #[test]
  fn going_back_does_not_add_a_place() {
    let mut model = model();
    visit(&mut model, Model::open_linear);
    model.go(true);
    model.record_history();
    assert!(model.history.can_go(false));
  }

  #[test]
  fn a_new_place_drops_the_ones_ahead() {
    let mut model = model();
    visit(&mut model, Model::open_linear);
    model.go(true);
    visit(&mut model, Model::open_github);
    assert!(!model.history.can_go(false));
    model.go(true);
    assert_eq!(model.page, Page::Workspace);
  }

  #[test]
  fn reloading_a_pull_request_is_not_a_new_place() {
    let mut model = model();
    visit(&mut model, |m| m.open_github_pr(1, "OPEN"));
    visit(&mut model, |m| {
      if let Page::Github(github) = &mut m.page {
        github.reload += 1;
      }
    });
    model.go(true);
    assert_eq!(model.page, Page::Workspace);
  }

  #[test]
  fn keeps_the_last_hundred_places() {
    let mut model = model();
    for id in 0..150 {
      visit(&mut model, |m| m.set_selected(Some(id)));
    }
    for _ in 0..200 {
      model.go(true);
    }
    assert_eq!(model.selected, Some(50));
  }
}
