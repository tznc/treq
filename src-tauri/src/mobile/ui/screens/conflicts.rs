//! Conflicted files, and resolving them by keeping one side or both.

use crate::core::remote::TreqCommandRequest;
use crate::mobile::ui::widgets::TouchButton as _;
use gpui_kit::{component::Selectable as _, prelude::FluentBuilder as _};
use gpui_kit::{
  component::{
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    Sizable,
  },
  div, px, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
  Render, ScrollHandle, StatefulInteractiveElement, Styled, Window,
};

use super::{mutation_message, on_epoch, workspace::Target};
use crate::mobile::{
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, nav, widgets, Load, Location},
};

pub fn open(target: Target, cx: &mut App) {
  let location = Location {
    repo: Some(target.repo.clone()),
    workspace: Some(crate::mobile::store::SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push("Conflicts", location, cx, move |window, cx| {
    cx.new(|cx| ConflictsScreen::new(target, window, cx))
  });
}

const SIDES: [(&str, &str); 3] = [
  ("side1", "Keep ours"),
  ("side2", "Keep theirs"),
  ("both", "Keep both"),
];

pub struct ConflictsScreen {
  target: Target,
  files: Load<Vec<String>>,
  revision: Entity<InputState>,
  side: usize,
  running: bool,
  /// An ambiguous resolve, for "Check again" (a read, never a resend).
  unsure: Option<(Vec<String>, TreqCommandRequest)>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  scroll: ScrollHandle,
}

impl ConflictsScreen {
  fn new(target: Target, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let revision = cx.new(|cx| {
      let mut state = InputState::new(window, cx).placeholder("Revision");
      state.set_value("@", window, cx);
      state
    });
    let mut this = Self {
      target,
      files: Load::Idle,
      revision,
      side: 0,
      running: false,
      unsure: None,
      message: None,
      keys: IdempotencyKeys::default(),
      scroll: ScrollHandle::new(),
    };
    this.reload(cx);
    this
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    self.files = Load::Loading;
    let request = TreqCommandRequest::ListConflicts {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
    };
    load::run(
      cx,
      remote::read::<Vec<String>>(request),
      |this, result, _| this.files = Load::from_result(result),
    );
  }

  fn resolve(&mut self, cx: &mut Context<Self>) {
    let revision = self.revision.read(cx).value().trim().to_string();
    let revision = if revision.is_empty() {
      "@".to_string()
    } else {
      revision
    };
    if self.running {
      return;
    }
    self.running = true;
    self.unsure = None;
    let side = SIDES[self.side].0.to_string();
    let inputs = [
      self.target.repo.clone(),
      self.target.id.clone(),
      revision.clone(),
      side.clone(),
    ];
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let request = TreqCommandRequest::ResolveConflict {
      repo: self.target.repo.clone(),
      revision,
      sides: vec![side],
      idempotency_key: self.keys.key_for("resolve", &refs),
    };
    let sent = request.clone();
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      this.running = false;
      let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
      this.keys.settle("resolve", &refs, &outcome);
      if matches!(outcome, Ok(Mutation::Ambiguous(_))) {
        this.unsure = Some((inputs.to_vec(), sent));
      }
      this.message = Some(mutation_message(&outcome, "Resolved."));
      if matches!(outcome, Ok(Mutation::Applied(_))) {
        this.reload(cx);
      }
    });
  }
}

impl ConflictsScreen {
  /// Reads whether the ambiguous resolve applied, without resending it.
  fn check_again(&mut self, cx: &mut Context<Self>) {
    if self.running {
      return;
    }
    let Some((inputs, request)) = self.unsure.take() else {
      return;
    };
    self.running = true;
    let sent = request.clone();
    load::run(
      cx,
      remote::check_again(request),
      move |this, outcome, cx| {
        this.running = false;
        let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        this.keys.settle("resolve", &refs, &outcome);
        if matches!(outcome, Ok(Mutation::Ambiguous(_))) {
          this.unsure = Some((inputs.clone(), sent));
        }
        this.message = Some(mutation_message(&outcome, "Resolved."));
        this.reload(cx);
      },
    );
  }
}

impl Render for ConflictsScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let has_conflicts = self.files.ready().is_some_and(|f| !f.is_empty());
    let files = match &self.files {
      Load::Idle | Load::Loading => widgets::card(cx).child(widgets::skeleton("conflicts", 3, cx)),
      Load::Failed(error) => widgets::card(cx).child(widgets::failed(
        "conflicts-failed",
        error,
        cx.listener(|this, _, _, cx| this.reload(cx)),
        cx,
      )),
      Load::Ready(files) if files.is_empty() => widgets::card(cx).child(widgets::empty(
        "conflicts-empty",
        "No conflicted files.",
        cx,
      )),
      Load::Ready(files) => widgets::card(cx).children(files.iter().map(|path| {
        h_flex()
          .min_h(px(widgets::ROW_H))
          .px_4()
          .items_center()
          .border_b_1()
          .child(widgets::mono(path.clone(), cx))
      })),
    };
    let label = if self.running {
      "Working…"
    } else {
      "Resolve"
    };
    let side = self.side;
    let resolve = widgets::card(cx)
      .p_4()
      .gap_3()
      .child(Input::new(&self.revision).large())
      .child(
        ButtonGroup::new("sides")
          .outline()
          .children(
            SIDES
              .iter()
              .enumerate()
              .map(|(i, (_, label))| Button::new(("side", i)).label(*label).selected(i == side)),
          )
          .on_click(cx.listener(|this, selected: &Vec<usize>, _, cx| {
            if let Some(&i) = selected.first() {
              this.side = i;
              cx.notify();
            }
          })),
      )
      .child(
        h_flex().child(
          Button::new("resolve")
            .primary()
            .large()
            .label(label)
            .loading(self.running)
            .on_click(cx.listener(|this, _, _, cx| {
              this.resolve(cx);
              cx.notify();
            }))
            .touch(),
        ),
      )
      .children(self.message.clone().map(|(ok, text)| {
        if ok {
          widgets::muted(text, cx)
        } else {
          widgets::error_text(text, cx)
        }
      }))
      .when(self.unsure.is_some(), |card| {
        card.child(
          h_flex().child(
            Button::new("check-again")
              .outline()
              .label("Check again")
              .on_click(cx.listener(|this, _, _, cx| {
                this.check_again(cx);
                cx.notify();
              }))
              .touch(),
          ),
        )
      });
    div()
      .id("conflicts")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(widgets::section("Conflicted files", cx).child(files))
          .when(has_conflicts, |p| {
            p.child(widgets::section("Resolve", cx).child(resolve))
          }),
      )
  }
}
