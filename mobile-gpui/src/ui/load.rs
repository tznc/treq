//! Running backend work from a view and applying its result.

use std::future::Future;

use gpui_kit::{Context, Window};

use crate::backend;

/// Data a screen fetches from the host.
pub enum Load<T> {
  Idle,
  Loading,
  Ready(T),
  Failed(String),
}

impl<T> Load<T> {
  pub fn is_loading(&self) -> bool {
    matches!(self, Self::Loading)
  }

  pub fn ready(&self) -> Option<&T> {
    match self {
      Self::Ready(value) => Some(value),
      _ => None,
    }
  }

  pub fn error(&self) -> Option<&str> {
    match self {
      Self::Failed(error) => Some(error),
      _ => None,
    }
  }

  pub fn from_result(result: Result<T, String>) -> Self {
    match result {
      Ok(value) => Self::Ready(value),
      Err(error) => Self::Failed(error),
    }
  }
}

/// Runs `future` on the backend runtime, then `done` on the view.
pub fn run<V: 'static, T: Send + 'static>(
  cx: &mut Context<V>,
  future: impl Future<Output = T> + Send + 'static,
  done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
  let result = backend::spawn(future);
  cx.spawn(async move |this, cx| {
    let Ok(value) = result.await else { return };
    this
      .update(cx, |view, cx| {
        done(view, value, cx);
        cx.notify();
      })
      .ok();
  })
  .detach();
}

/// Like [`run`], with the window (to update text inputs, for example).
pub fn run_in<V: 'static, T: Send + 'static>(
  window: &Window,
  cx: &mut Context<V>,
  future: impl Future<Output = T> + Send + 'static,
  done: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
) {
  let result = backend::spawn(future);
  cx.spawn_in(window, async move |this, cx| {
    let Ok(value) = result.await else { return };
    this
      .update_in(cx, |view, window, cx| {
        done(view, value, window, cx);
        cx.notify();
      })
      .ok();
  })
  .detach();
}
