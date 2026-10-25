//! Draws [`crate::core::context_menu`]'s menus with `popup`, and runs the
//! picks every menu runs the same way: copying, revealing and opening.

use std::rc::Rc;

use tauri_plugin_gpui::gpui::{App, ClipboardItem};

use super::{
  popup::{self, MenuEntry, MenuIcon, NativeMenuItem},
  screen::ChangesHost,
};
use crate::core::context_menu::{self, MenuAction, MenuItem};

/// Runs a pick.
pub type RunAction = Rc<dyn Fn(MenuAction, &mut App)>;

fn icon(icon: context_menu::MenuIcon) -> MenuIcon {
  match icon {
    context_menu::MenuIcon::Archive => MenuIcon::Archive,
    context_menu::MenuIcon::Copy => MenuIcon::Copy,
    context_menu::MenuIcon::FolderOpen => MenuIcon::FolderOpen,
    context_menu::MenuIcon::FolderPlus => MenuIcon::FolderPlus,
    context_menu::MenuIcon::FolderSearch => MenuIcon::FolderSearch,
    context_menu::MenuIcon::GitBranch => MenuIcon::GitBranch,
    context_menu::MenuIcon::Link => MenuIcon::Link,
    context_menu::MenuIcon::Pencil => MenuIcon::Pencil,
    context_menu::MenuIcon::RotateCcw => MenuIcon::RotateCcw,
    context_menu::MenuIcon::Settings => MenuIcon::Settings,
    context_menu::MenuIcon::Trash => MenuIcon::Trash2,
  }
}

fn native(items: Vec<MenuItem>, run: &RunAction) -> Vec<NativeMenuItem> {
  items
    .into_iter()
    .map(|item| match item {
      MenuItem::Separator => NativeMenuItem::Separator,
      MenuItem::Submenu {
        text,
        icon: glyph,
        items,
      } => NativeMenuItem::Submenu {
        text,
        icon: glyph.map(icon),
        detail: None,
        items: native(items, run),
      },
      MenuItem::Item {
        text,
        icon: glyph,
        destructive,
        action,
      } => {
        let run = run.clone();
        let mut item = NativeMenuItem::item(text, move |cx| run(action.clone(), cx));
        if let Some(glyph) = glyph {
          item = item.icon(icon(glyph));
        }
        if destructive {
          item = item.destructive();
        }
        item
      }
    })
    .collect()
}

/// `items` as popup menu entries whose picks go to `run`. Main thread only:
/// building a menu replaces the last one's handlers.
pub fn entries(items: Vec<MenuItem>, run: RunAction) -> Vec<MenuEntry> {
  popup::native_entries(native(items, &run))
}

/// Runs the picks that need nothing of the menu's owner: copying a text,
/// revealing a path and opening a URL. Returns any other action.
pub fn run_common(action: MenuAction, host: &dyn ChangesHost, cx: &mut App) -> Option<MenuAction> {
  match action {
    MenuAction::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
    MenuAction::Reveal(path) => host.reveal(&path),
    MenuAction::OpenUrl(url) => host.open_url(&url),
    action => return Some(action),
  }
  None
}
