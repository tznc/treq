//! Keyboard movement through a list: the one place a key becomes a
//! selection step, so every list answers ↑/↓, Home/End, PageUp/PageDown and
//! Enter the same way.

use gpui::Modifiers;

/// What a key asks of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
  /// ↑.
  Prev,
  /// ↓.
  Next,
  /// Home, or Cmd+↑.
  First,
  /// End, or Cmd+↓.
  Last,
  /// PageUp.
  PagePrev,
  /// PageDown.
  PageNext,
  /// Enter: open the selected item.
  Open,
}

impl ListKey {
  /// The list key `key` is with `held` down (a desktop screen passes the
  /// modifiers through `keys::held`). Shift and Alt don't change it; callers
  /// that give them a meaning of their own check them.
  pub fn from_key(key: &str, held: Modifiers) -> Option<Self> {
    Some(match key {
      "up" if held.platform => Self::First,
      "down" if held.platform => Self::Last,
      "up" => Self::Prev,
      "down" => Self::Next,
      "home" => Self::First,
      "end" => Self::Last,
      "pageup" => Self::PagePrev,
      "pagedown" => Self::PageNext,
      "enter" => Self::Open,
      _ => return None,
    })
  }

  /// [`ListKey::from_key`], plus Ctrl+N/J for ↓ and Ctrl+P/K for ↑, for a
  /// filtered list whose search field has the keyboard.
  pub fn from_key_with_emacs(key: &str, held: Modifiers) -> Option<Self> {
    match key {
      "n" | "j" if held.control => Some(Self::Next),
      "p" | "k" if held.control => Some(Self::Prev),
      _ => Self::from_key(key, held),
    }
  }

  /// ↑ and ↓ as a step: -1 and 1; `None` for the other keys.
  pub fn delta(self) -> Option<isize> {
    match self {
      Self::Prev => Some(-1),
      Self::Next => Some(1),
      _ => None,
    }
  }
}

/// Moves a selection through `len` items. Steps stop at the ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListNav {
  len: usize,
  page: usize,
}

/// How many items PageUp and PageDown skip.
pub const PAGE: usize = 10;

impl ListNav {
  pub fn new(len: usize) -> Self {
    Self { len, page: PAGE }
  }

  /// The index selected after `key` from `selected`; `None` while the list
  /// is empty or nothing is selected and the key doesn't pick an item.
  pub fn moved(&self, selected: Option<usize>, key: ListKey) -> Option<usize> {
    let last = self.len.checked_sub(1)?;
    let at = selected.map(|s| s.min(last));
    match key {
      ListKey::Prev => at.map(|a| a.saturating_sub(1)),
      ListKey::Next => Some(at.map_or(0, |a| (a + 1).min(last))),
      ListKey::First => Some(0),
      ListKey::Last => Some(last),
      ListKey::PagePrev => at.map(|a| a.saturating_sub(self.page)),
      ListKey::PageNext => Some(at.map_or(0, |a| (a + self.page).min(last))),
      ListKey::Open => at,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn key(s: &str) -> Option<ListKey> {
    let k = gpui::Keystroke::parse(s).unwrap();
    ListKey::from_key(&k.key, k.modifiers)
  }

  #[test]
  fn keys_map_to_list_moves() {
    assert_eq!(key("up"), Some(ListKey::Prev));
    assert_eq!(key("down"), Some(ListKey::Next));
    assert_eq!(key("shift-down"), Some(ListKey::Next));
    assert_eq!(key("home"), Some(ListKey::First));
    assert_eq!(key("end"), Some(ListKey::Last));
    assert_eq!(key("cmd-up"), Some(ListKey::First));
    assert_eq!(key("cmd-down"), Some(ListKey::Last));
    assert_eq!(key("pageup"), Some(ListKey::PagePrev));
    assert_eq!(key("pagedown"), Some(ListKey::PageNext));
    assert_eq!(key("enter"), Some(ListKey::Open));
    assert_eq!(key("left"), None);
    assert_eq!(
      key("ctrl-n"),
      None,
      "only a search field's list takes Ctrl+N"
    );
    let emacs = |s| {
      let k = gpui::Keystroke::parse(s).unwrap();
      ListKey::from_key_with_emacs(&k.key, k.modifiers)
    };
    assert_eq!(emacs("ctrl-n"), Some(ListKey::Next));
    assert_eq!(emacs("ctrl-j"), Some(ListKey::Next));
    assert_eq!(emacs("ctrl-p"), Some(ListKey::Prev));
    assert_eq!(emacs("ctrl-k"), Some(ListKey::Prev));
    assert_eq!(emacs("n"), None);
    assert_eq!(emacs("down"), Some(ListKey::Next));
  }

  #[test]
  fn up_at_the_top_and_down_at_the_bottom_stay() {
    let nav = ListNav::new(3);
    assert_eq!(nav.moved(Some(0), ListKey::Prev), Some(0));
    assert_eq!(nav.moved(Some(2), ListKey::Next), Some(2));
    assert_eq!(nav.moved(Some(0), ListKey::Next), Some(1));
    assert_eq!(nav.moved(Some(2), ListKey::Prev), Some(1));
  }

  #[test]
  fn down_with_nothing_selected_picks_the_first() {
    let nav = ListNav::new(3);
    assert_eq!(nav.moved(None, ListKey::Next), Some(0));
    assert_eq!(nav.moved(None, ListKey::Prev), None);
    assert_eq!(ListNav::new(0).moved(None, ListKey::Next), None);
  }

  #[test]
  fn home_and_end_jump_to_the_ends() {
    let nav = ListNav::new(5);
    assert_eq!(nav.moved(Some(2), ListKey::First), Some(0));
    assert_eq!(nav.moved(Some(2), ListKey::Last), Some(4));
    assert_eq!(nav.moved(None, ListKey::Last), Some(4));
    assert_eq!(ListNav::new(0).moved(None, ListKey::First), None);
  }

  #[test]
  fn page_keys_skip_a_page_and_stop_at_the_ends() {
    let nav = ListNav::new(25);
    assert_eq!(nav.moved(Some(3), ListKey::PageNext), Some(13));
    assert_eq!(nav.moved(Some(20), ListKey::PageNext), Some(24));
    assert_eq!(nav.moved(Some(13), ListKey::PagePrev), Some(3));
    assert_eq!(nav.moved(Some(3), ListKey::PagePrev), Some(0));
  }

  #[test]
  fn enter_keeps_the_selection() {
    assert_eq!(ListNav::new(3).moved(Some(1), ListKey::Open), Some(1));
  }
}
