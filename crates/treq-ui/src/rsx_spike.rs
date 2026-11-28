//! Spike: does gpui-rsx compose with treq's `.selector()` and click handlers?

#[cfg(test)]
mod tests {
  use std::{cell::Cell, rc::Rc};

  use gpui::{div, prelude::*, px, Context, TestAppContext, Window};
  use gpui_rsx::rsx;

  use crate::SelectorExt as _;

  struct View {
    clicks: Rc<Cell<u32>>,
  }

  impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      let clicks = self.clicks.clone();
      rsx! {
        <div size_full>
          <div
            selector="go"
            size={px(20.)}
            onClick={move |_, _, _| clicks.set(clicks.get() + 1)}
          />
        </div>
      }
    }
  }

  #[gpui::test]
  fn selector_and_on_click_in_rsx_keep_the_selector_id(cx: &mut TestAppContext) {
    let clicks = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view({
      let clicks = clicks.clone();
      move |_, _| View { clicks }
    });
    cx.run_until_parked();
    let bounds = cx.debug_bounds("go").expect("selector `go` is drawn");
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    assert_eq!(clicks.get(), 1);
  }
}
