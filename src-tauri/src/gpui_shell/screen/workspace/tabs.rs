//! The tab views the workspace view keeps: Commits, Changes and the file
//! browser, one per workspace each. A tab or workspace shown again draws
//! what it last read at once and reads again behind it; a workspace shown
//! for the first time keeps the last one drawn until its first read lands,
//! so a switch never flashes a loading placeholder. A view can also be made
//! ahead of a switch (`prefetch`), so the switch draws it read already.

use tauri_plugin_gpui::gpui::{App, Context, Entity, EntityId, Subscription};

use super::{
  super::{changes::ChangesView, commits::CommitsView, file_browser::FileBrowserView},
  WorkspaceView,
};

/// How many workspaces' views each tab keeps, the shown one included.
const KEPT: usize = 4;

/// What a tab view is for (`Parent::key`).
pub(super) type Key = (String, Option<i64>);

/// A tab view kept while hidden.
pub(super) trait TabView: Sized + 'static {
  /// Its first read landed, so it draws content rather than a placeholder.
  fn ready(&self) -> bool;
  /// Hidden (`false`), or shown again: a hidden view stops polling, and
  /// one shown again reads again, drawing what it has meanwhile.
  fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>);
}

/// One tab's views by workspace.
pub(super) struct Views<V: TabView> {
  /// Most recently asked for last.
  views: Vec<(Key, Entity<V>)>,
  /// Drawn in place of the newest view until its first read lands: the
  /// view drawn before it, of another workspace.
  stand_in: Option<Entity<V>>,
  /// Redraws the workspace view once the newest view is ready.
  _ready: Option<Subscription>,
}

impl<V: TabView> Default for Views<V> {
  fn default() -> Self {
    Self {
      views: Vec::new(),
      stand_in: None,
      _ready: None,
    }
  }
}

impl<V: TabView> Views<V> {
  /// The newest view: the one the tab draws, once ready.
  pub fn newest(&self) -> Option<&Entity<V>> {
    self.views.last().map(|(_, view)| view)
  }

  /// `key`'s view, made with `make` when the tab has none for it. When
  /// `stand_in` allows it, the view drawn until now stays drawn until the
  /// new one is ready.
  pub fn view(
    &mut self,
    key: Key,
    stand_in: bool,
    make: impl FnOnce(&mut Context<WorkspaceView>) -> Entity<V>,
    cx: &mut Context<WorkspaceView>,
  ) -> Entity<V> {
    if let Some(index) = self.views.iter().position(|(k, _)| *k == key) {
      if index + 1 < self.views.len() {
        // Back to a workspace shown before, or made ahead: it draws what it
        // read, or stands the last one in while its read is still on the way.
        let drawn = self.drawn(cx).map(|(view, _)| view);
        let entry = self.views.remove(index);
        let view = entry.1.clone();
        self.views.push(entry);
        let stand_in = stand_in && !view.read(cx).ready();
        self.stand_in_until_ready(drawn.filter(|_| stand_in), &view, cx);
      }
      return self.views[self.views.len() - 1].1.clone();
    }
    let drawn = self.drawn(cx).map(|(view, _)| view);
    let view = make(cx);
    self.stand_in_until_ready(drawn.filter(|_| stand_in), &view, cx);
    self.views.push((key, view.clone()));
    if self.views.len() > KEPT {
      self.views.remove(0);
    }
    view
  }

  /// Draws `stand_in` in place of `view` until `view` is ready.
  fn stand_in_until_ready(
    &mut self,
    stand_in: Option<Entity<V>>,
    view: &Entity<V>,
    cx: &mut Context<WorkspaceView>,
  ) {
    self.stand_in = stand_in;
    self._ready = self
      .stand_in
      .is_some()
      .then(|| cx.observe(view, |_, _, cx| cx.notify()));
  }

  /// Makes `key`'s view, hidden, before the tab asks for it, unless it has
  /// one already, so showing it draws what it read. It goes below the
  /// newest view, so what the tab draws stays.
  pub fn prefetch(
    &mut self,
    key: Key,
    make: impl FnOnce(&mut Context<WorkspaceView>) -> Entity<V>,
    cx: &mut Context<WorkspaceView>,
  ) {
    if self.views.iter().any(|(k, _)| *k == key) {
      return;
    }
    let view = make(cx);
    view.update(cx, |view, cx| view.set_shown(false, cx));
    let at = self.views.len().saturating_sub(1);
    self.views.insert(at, (key, view));
    if self.views.len() > KEPT {
      self.views.remove(0);
    }
  }

  /// Whether `key`'s view, if the tab keeps one, has read.
  pub fn ready_for(&self, key: &Key, cx: &App) -> bool {
    self
      .views
      .iter()
      .filter(|(k, _)| k == key)
      .all(|(_, view)| view.read(cx).ready())
  }

  /// What the tab draws: the newest view once ready, else its stand-in
  /// (`true`).
  pub fn drawn(&mut self, cx: &App) -> Option<(Entity<V>, bool)> {
    let newest = self.newest()?.clone();
    if let Some(stand_in) = &self.stand_in {
      if !newest.read(cx).ready() {
        return Some((stand_in.clone(), true));
      }
    }
    self.stand_in = None;
    self._ready = None;
    Some((newest, false))
  }

  /// Hides every view but `drawn` and the newest (which is either drawn or
  /// about to be), and shows `drawn`.
  pub fn show_only(&self, drawn: Option<EntityId>, cx: &mut App) {
    let newest = self.newest().map(|view| view.entity_id());
    for (_, view) in &self.views {
      let id = view.entity_id();
      if Some(id) == newest && drawn != newest && self.stand_in.is_some() {
        continue;
      }
      let shown = Some(id) == drawn;
      view.update(cx, |view, cx| view.set_shown(shown, cx));
    }
  }

  /// Whether `id` is one of the tab's views.
  pub fn has(&self, id: EntityId) -> bool {
    self.views.iter().any(|(_, view)| view.entity_id() == id)
  }

  pub fn clear(&mut self) {
    *self = Self::default();
  }
}

impl TabView for CommitsView {
  fn ready(&self) -> bool {
    CommitsView::ready(self)
  }

  fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
    CommitsView::set_shown(self, shown, cx);
  }
}

impl TabView for ChangesView {
  fn ready(&self) -> bool {
    ChangesView::ready(self)
  }

  fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
    ChangesView::set_shown(self, shown, cx);
  }
}

impl TabView for FileBrowserView {
  fn ready(&self) -> bool {
    FileBrowserView::ready(self)
  }

  fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
    FileBrowserView::set_shown(self, shown, cx);
  }
}
