//! Treq's design system, built with gpui-styleguidist.
//!
//! - `cargo run -p treq-ui --example styleguide` opens it.
//! - `-- render` writes snapshots under `styleguide/` (git-ignored; not
//!   checked in); `-- check` compares against a local render.

use gpui::{div, prelude::*, px, App, Entity, Window};
use gpui_kit::component::input::InputState;
use gpui_styleguidist::{example, Example, Styleguide};
use treq_ui::{
  floating,
  theme::{self, Palette},
  touch, Alert, Badge, Button, Card, CardFooter, CardHeader, CardVariant, CheckState, Checkbox,
  ConfirmDialog, Count, Dialog, Dot, EmptyState, Field, FieldLayout, Form, FormActions, FormLayout,
  FormRow, FormSection, IconButton, IconButtonSize, Kbd, Loading, MenuItem, MenuLabel,
  MenuSeparator, PathField, Picker, PickerPlacement, Row, RowDensity, Severity, Skeleton,
  SplitButton, Tab, Tabs, TabsStyle, Tone, Variant,
};

fn main() {
  gpui_styleguidist::main(styleguide());
}

fn p(cx: &App) -> Palette {
  theme::current(cx)
}

fn styleguide() -> Styleguide {
  Styleguide::new("Treq design system")
    .assets(theme::Assets)
    .setup(theme::init)
    .open_window(|options, frame, cx| Ok(gpui_kit::open_window(options, cx, frame)?.0))
    // Every example also renders at touch density, so each desktop
    // component has its phone translation in the snapshots.
    .themes(["light", "dark", "touch-light", "touch-dark"], |name, cx| {
      let touch = name.starts_with("touch");
      theme::set_density(
        if touch {
          theme::Density::Touch
        } else {
          theme::Density::Desktop
        },
        cx,
      );
      theme::set_dark(name.ends_with("dark"), cx)
    })
    .background(|cx| p(cx).background)
    // Past this, snapshots cover the default plus each single-knob change.
    .variant_limit(6)
    .describe(
      "Button",
      "One primary (filled) button per region, bound to Enter. Outline for values and \
       secondary actions, ghost for toolbar commands, destructive for what can't be undone. \
       Labels are verbs, with \"…\" when the button opens more UI.",
    )
    .describe(
      "IconButton",
      "An icon-only button, 24 in rows and 28 in toolbars. Its label is required: it is the \
       accessible name and the tooltip. A tone colours the icon for state, such as danger.",
    )
    .describe(
      "Copy feedback",
      "A copy button's icon turns into a green check for 2s after it copies, fading and \
       scaling between the two. Wrap the control in copy_feedback::copyable, which keeps the \
       state, and draw the icon with Button::copy_icon or IconButton::copied. A label that \
       says \"Copy\" may read \"Copied\" while the check shows. Keep any toast the copy \
       already shows.",
    )
    .describe(
      "SplitButton",
      "A command with a menu of related commands. One silhouette; the menu half has its own \
       accessible name. The pill variant is a state that is also a button (a PR's Open): \
       fully rounded, tinted in the state's tone like a Badge, its second half a related \
       command such as open on the web.",
    )
    .describe(
      "Picker",
      "A value that can change: outline with a trailing chevron. A command that opens a menu \
       is a Button with no chevron.",
    )
    .describe(
      "Badge",
      "Badge labels an object, Count says how many, Dot gives one status. Counts are always \
       neutral; colour means state.",
    )
    .describe(
      "Kbd",
      "Shortcuts in the platform's names with \" + \" between keys: ⌘ + K on macOS, Ctrl + K elsewhere.",
    )
    .describe(
      "Alert",
      "The icon and tint carry the severity; the title states the fact, the message says \
       what to do. Actions sit bottom right under the text: one outline verb, or a ghost \
       dismiss before it when there are two.",
    )
    .describe(
      "Checkbox",
      "13px, primary when on, a dash for a partly checked parent. The label sits right of \
       the box in the row that takes the click, in body text; a hint goes under it, muted.",
    )
    .describe(
      "Tabs",
      "Segmented switches views of one thing; underline switches panels. A badge labels \
       the tab (a plan chip), counts trail the label, and a problem is a dot.",
    )
    .describe(
      "Row",
      "28 in the sidebar and trees, 32 in dialogs, two-line for commits and PRs. One hover \
       fill, one selected fill, status as a trailing dot.",
    )
    .describe(
      "Menu",
      "Rows 28 tall: icon, label, shortcut; a description adds a muted second line. Values \
       get a check under a group label that names the value; destructive items go last.",
    )
    .describe(
      "Dialog",
      "Radius 12 on the scrim. An action pair goes in the footer with no X; otherwise an X \
       closes it. Confirmations ask a question and answer with verbs, never Yes/No.",
    )
    .describe(
      "Floating",
      "A surface in the middle of the window that is not a dialog: the palette, a file \
       finder. Radius 12, a border and the deepest shadow; the caller picks the fill.",
    )
    .describe(
      "EmptyState",
      "Says why it's empty and what to do, with at most one action. Loading shows where the \
       result will appear.",
    )
    .examples(buttons())
    .examples(indicators())
    .examples(navigation())
    .describe(
      "Card",
      "One object per card. Outlined by default; elevated for a card that floats over the \
       page, never nested; ghost for an empty slot. A footer only when there are actions, \
       the primary last. Flush for a list edge to edge.",
    )
    .describe(
      "Form",
      "Vertical in dialogs and narrow panes; horizontal (a 140 label column) in settings \
       pages, stacking below 480; inline for filter bars. Rows split a line by span; \
       sections group fields under a title. Actions end-align, or line up with the \
       controls in a horizontal form.",
    )
    .describe(
      "Field",
      "A label, its control and one line under it: the hint, or the error that replaces \
       it. All body text, so they line up with the text in the control. A red * for \
       required; \"(optional)\" when most fields are required. Trailing puts the control \
       at the end of the row, for a setting.",
    )
    .describe(
      "PathField",
      "Paths are picked, not typed: the value in mono and Browse…, which opens the \
       system's picker. Dropping a folder on it picks that.",
    )
    .examples(surfaces())
    .describe(
      "InputGroup",
      "A field with icon buttons joined to its end in one border, for a value that comes \
       from an action (a picker, a lookup). Preferred on a phone over a text button \
       beside the field. Each icon button's label names the action.",
    )
    .describe(
      "Mobile",
      "Phone-only patterns. Every other page also renders at touch density (the touch \
       themes): 44 targets, forms vertical, actions full width, no shortcuts.",
    )
    .examples(cards())
    .examples(forms())
    .examples(mobile())
}

fn cards() -> Vec<Example> {
  vec![
    example!("card/variants", |knobs, _, cx| {
      let p = p(cx);
      div().w(px(360.)).child(
        Card::new("card", &p)
          .variant(knobs.pick(
            "variant",
            &[
              ("outlined", CardVariant::Outlined),
              ("elevated", CardVariant::Elevated),
              ("ghost", CardVariant::Ghost),
            ],
            "outlined",
          ))
          .selected(knobs.bool("selected", false))
          .header(
            CardHeader::new("treq")
              .description("~/code/treq")
              .action(IconButton::new("card-more", theme::ICON_MORE, "More", &p)),
          )
          .child("3 workspaces, 1 with uncommitted changes.")
          .footer(
            CardFooter::new()
              .end(
                Button::new("card-cancel", &p)
                  .variant(Variant::Ghost)
                  .label("Cancel"),
              )
              .end(Button::new("card-open", &p).label("Open")),
          ),
      )
    })
    .component("Card")
    .title("Variants")
    .size(440., 220.),
    example!("card/flush", |_, _, cx| {
      let p = p(cx);
      div().w(px(360.)).child(
        Card::new("branches", &p)
          .header(CardHeader::new("Branches").action(Count::new(3, &p)))
          .flush(true)
          .children(["main", "feat/cards", "fix/forms"].map(|b| Row::new(b, &p).title(b)))
          .footer(
            CardFooter::new()
              .start(
                Button::new("b-delete", &p)
                  .variant(Variant::DestructiveOutline)
                  .label("Delete"),
              )
              .end(
                Button::new("b-open", &p)
                  .variant(Variant::Outline)
                  .label("Open"),
              ),
          ),
      )
    })
    .component("Card")
    .title("Flush list, split footer")
    .size(440., 260.),
  ]
}

/// Inputs need state, so form examples are views.
struct FormDemo {
  layout: FormLayout,
  error: bool,
  name: Entity<InputState>,
  email: Entity<InputState>,
  owner: Entity<InputState>,
  repo: Entity<InputState>,
  depth: Entity<InputState>,
}

impl FormDemo {
  fn new(
    layout: FormLayout,
    error: bool,
    window: &mut Window,
    cx: &mut gpui::Context<Self>,
  ) -> Self {
    let mut input = |v: &str| {
      let v = v.to_string();
      cx.new(|cx| InputState::new(window, cx).default_value(v))
    };
    Self {
      layout,
      error,
      name: input("feat/card-system"),
      email: input("ty@example"),
      owner: input("tznc"),
      repo: input("treq"),
      depth: input("50"),
    }
  }
}

impl Render for FormDemo {
  fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
    let p = p(cx);
    let form = Form::new("demo", &p).layout(self.layout);
    let form = if self.layout == FormLayout::Inline {
      form
        .field(
          Field::new("owner", &p)
            .label("Owner")
            .child(treq_ui::input(&self.owner)),
        )
        .field(
          Field::new("repo", &p)
            .label("Name")
            .child(treq_ui::input(&self.repo)),
        )
        .child(
          Button::new("reset", &p)
            .variant(Variant::Outline)
            .label("Reset"),
        )
    } else {
      form
        .section(
          FormSection::new("Workspace")
            .field(
              Field::new("name", &p)
                .label("Name")
                .required()
                .hint("Becomes the branch name.")
                .child(treq_ui::input(&self.name)),
            )
            .field(
              Field::new("email", &p)
                .label("Email")
                .hint("Used for commit attribution.")
                .error(
                  self
                    .error
                    .then_some("Use a full address, such as ty@example.com."),
                )
                .child(treq_ui::input(&self.email)),
            )
            .field(
              Field::new("path", &p).label("Clone path").child(
                PathField::new("path-picker", &p)
                  .value(Some("~/code/treq"))
                  .on_pick(|_, _, _| {}),
              ),
            ),
        )
        .section(
          FormSection::new("Repository").row(
            FormRow::new()
              .span(
                2,
                Field::new("owner", &p)
                  .label("Owner")
                  .child(treq_ui::input(&self.owner)),
              )
              .col(
                Field::new("depth", &p)
                  .label("Depth")
                  .child(treq_ui::input(&self.depth)),
              ),
          ),
        )
        .field(
          Field::new("open", &p).child(
            Checkbox::new("open-after", &p)
              .checked(true)
              .label("Open after creating"),
          ),
        )
        .actions(
          FormActions::new()
            .end(
              Button::new("f-cancel", &p)
                .variant(Variant::Ghost)
                .label("Cancel"),
            )
            .end(Button::new("f-create", &p).label("Create workspace")),
        )
    };
    div().w(px(560.)).child(form)
  }
}

fn forms() -> Vec<Example> {
  vec![
    example!(view "form/layouts", |knobs, window, cx| {
      let layout = knobs.pick(
        "layout",
        &[
          ("vertical", FormLayout::Vertical),
          ("horizontal", FormLayout::horizontal()),
          ("inline", FormLayout::Inline),
        ],
        "vertical",
      );
      let error = knobs.bool("error", false);
      cx.new(|cx| FormDemo::new(layout, error, window, cx))
    })
    .component("Form")
    .title("Vertical, horizontal, inline")
    .description("Sections, a 2:1 row, a path picker and a checkbox in one form.")
    .size(640., 520.),
    example!("field/settings", |_, _, cx| {
      let p = p(cx);
      div().w(px(420.)).child(
        Card::new("settings", &p).flush(true).child(
          div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(
              Field::new("compact", &p)
                .layout(FieldLayout::Trailing)
                .label("Compact rows")
                .hint("28px rows in the sidebar.")
                .child(Checkbox::new("compact-switch", &p).switch().checked(true)),
            )
            .child(
              Field::new("signout", &p)
                .layout(FieldLayout::Trailing)
                .label("Sign out everywhere")
                .child(
                  Button::new("signout-btn", &p)
                    .variant(Variant::DestructiveOutline)
                    .label("Sign out…"),
                ),
            ),
        ),
      )
    })
    .component("Field")
    .title("Settings (trailing)")
    .size(500., 200.),
    example!("checkbox/radio-switch", |_, _, cx| {
      let p = p(cx);
      div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
          Checkbox::new("r1", &p)
            .radio()
            .checked(true)
            .label("Merge commit"),
        )
        .child(Checkbox::new("r2", &p).radio().label("Squash and merge"))
        .child(
          Checkbox::new("s1", &p)
            .switch()
            .checked(true)
            .label("Delete branch after merge"),
        )
        .child(Checkbox::new("s2", &p).switch().label("Auto-merge"))
    })
    .component("Checkbox")
    .title("Radio and switch")
    .size(360., 160.),
    example!("path-field", |knobs, _, cx| {
      let p = p(cx);
      div().w(px(360.)).child(
        PathField::new("clone", &p)
          .value(knobs.bool("chosen", true).then_some("~/code/treq"))
          .disabled(knobs.bool("disabled", false))
          .on_pick(|_, _, _| {}),
      )
    })
    .component("PathField")
    .title("Folder")
    .size(440., 80.),
  ]
}

fn variant_knob(knobs: &gpui_styleguidist::Knobs) -> Variant {
  knobs.pick(
    "variant",
    &[
      ("default", Variant::Default),
      ("outline", Variant::Outline),
      ("ghost", Variant::Ghost),
      ("destructive", Variant::Destructive),
      ("warning", Variant::Warning),
      ("secondary", Variant::Secondary),
      ("link", Variant::Link),
    ],
    "default",
  )
}

// Examples with a transparent edge (ghost buttons, empty states) render at a
// fixed size: cropping to drawn pixels would depend on anti-aliasing.
fn buttons() -> Vec<Example> {
  vec![
    example!("button/playground", |knobs, _, cx| {
      let p = p(cx);
      Button::new("button", &p)
        .variant(variant_knob(knobs))
        .size(treq_ui::Size::Sm)
        .icon(theme::ICON_CHECK, 16.)
        .label(knobs.text("label", "Merge"))
        .gap_1p5()
        .disabled(knobs.bool("disabled", false))
        .selected(knobs.bool("selected", false))
    })
    .component("Button")
    .title("Playground")
    .size(240., 64.),
    example!("button/variants", |knobs, _, cx| {
      let p = p(cx);
      let disabled = knobs.bool("disabled", false);
      div().flex().flex_wrap().gap_2().children(
        [
          ("Merge", Variant::Default),
          ("Rename…", Variant::Outline),
          ("Add", Variant::Ghost),
          ("Delete", Variant::Destructive),
          ("Resync", Variant::Warning),
          ("Open docs", Variant::Link),
        ]
        .map(|(label, variant)| {
          Button::new(label, &p)
            .variant(variant)
            .size(treq_ui::Size::Sm)
            .label(label)
            .disabled(disabled)
        }),
      )
    })
    .component("Button")
    .title("Variants"),
    example!("icon-button/states", |knobs, _, cx| {
      let p = p(cx);
      let size = knobs.pick(
        "size",
        &[
          ("medium", IconButtonSize::Medium),
          ("small", IconButtonSize::Small),
        ],
        "medium",
      );
      div()
        .flex()
        .items_center()
        .gap_2()
        .child(IconButton::new("ghost", theme::ICON_X, "Close", &p).size(size))
        .child(
          IconButton::new("outline", theme::ICON_CHEVRON_DOWN, "More", &p)
            .outline()
            .size(size),
        )
        .child(
          IconButton::new("selected", theme::ICON_INFO, "Details", &p)
            .selected(true)
            .size(size),
        )
        .child(
          IconButton::new("danger", theme::ICON_X, "Remove", &p)
            .tone(Tone::Danger)
            .size(size),
        )
        .child(
          IconButton::new("disabled", theme::ICON_X, "Close", &p)
            .disabled(true)
            .size(size),
        )
        .child(
          IconButton::new("primary", theme::ICON_SEND, "Send", &p)
            .primary()
            .size(size),
        )
    })
    .component("IconButton")
    .title("States")
    .size(268., 56.),
    example!("copy-feedback/states", |knobs, _, cx| {
      let p = p(cx);
      let copied = knobs.bool("copied", true);
      div()
        .flex()
        .items_center()
        .gap_2()
        .child(
          IconButton::new("icon", theme::ICON_COPY, "Copy file path", &p)
            .size(IconButtonSize::Small)
            .copied(copied),
        )
        .child(
          IconButton::new("outline", theme::ICON_COPY, "Copy file contents", &p)
            .size(IconButtonSize::Small)
            .outline()
            .copied(copied),
        )
        .child(
          Button::new("labelled", &p)
            .variant(Variant::Outline)
            .size(treq_ui::Size::Sm)
            .gap_2()
            .copy_icon(copied, 12.)
            .label(if copied { "Copied" } else { "Copy" }),
        )
    })
    .component("Copy feedback")
    .title("States")
    .size(232., 56.),
    example!("split-button/playground", |knobs, _, cx| {
      let p = p(cx);
      SplitButton::new("push", "Push", &p)
        .icon(theme::ICON_CHEVRON_UP)
        .when(knobs.bool("outline", false), |b| b.outline())
        .open(knobs.bool("open", false))
        .disabled(knobs.bool("disabled", false))
    })
    .component("SplitButton")
    .title("Playground"),
    example!("split-button/pill", |knobs, _, cx| {
      let p = p(cx);
      let (label, icon, tone) = knobs.pick(
        "state",
        &[
          ("open", ("Open", theme::ICON_PR, Tone::GitOpen)),
          ("draft", ("Draft", theme::ICON_PR_DRAFT, Tone::Neutral)),
          ("merged", ("Merged", theme::ICON_MERGE, Tone::GitMerged)),
          ("closed", ("Closed", theme::ICON_PR_CLOSED, Tone::GitClosed)),
        ],
        "open",
      );
      SplitButton::new("pr", label, &p)
        .pill(tone)
        .icon(icon)
        .aria_label(format!("View PR (#42, {})", label.to_lowercase()))
        .menu_label("Open PR on web")
        .menu_icon(theme::ICON_EXTERNAL_LINK)
    })
    .component("SplitButton")
    .title("Status pill")
    .size(160., 56.),
    example!("picker/placements", |knobs, _, cx| {
      let p = p(cx);
      let open = knobs.bool("open", false);
      div()
        .w(px(260.))
        .flex()
        .flex_col()
        .gap_2()
        .child(
          Picker::new("target", &p)
            .label("Target branch")
            .value("main")
            .open(open),
        )
        .child(
          Picker::new("empty", &p)
            .label("Agent")
            .placeholder("Pick an agent"),
        )
        .child(
          Picker::new("toolbar", &p)
            .label("Model")
            .value("Default")
            .placement(PickerPlacement::Toolbar),
        )
        .child(
          Picker::new("disabled", &p)
            .label("Target branch")
            .value("main")
            .disabled(true),
        )
    })
    .component("Picker")
    .title("Form and toolbar"),
  ]
}

fn indicators() -> Vec<Example> {
  vec![
    example!("badge/tones", |_, _, cx| {
      let p = p(cx);
      div().flex().flex_col().gap_3().children([
        div().flex().items_center().gap_2().children(
          [
            ("PRO", Tone::Neutral),
            ("Open", Tone::GitOpen),
            ("Merged", Tone::GitMerged),
            ("Closed", Tone::GitClosed),
            ("Draft", Tone::Neutral),
            ("Failing", Tone::Danger),
          ]
          .map(|(label, tone)| Badge::new(label, &p).tone(tone).into_any_element()),
        ),
        div()
          .flex()
          .items_center()
          .gap_2()
          .children([0, 3, 42, 1500].map(|n| Count::new(n, &p).into_any_element())),
        div().flex().items_center().gap_2().children(
          // Every dot names its state; hover one for its tooltip.
          [
            (Tone::Neutral, "Agent running"),
            (Tone::Primary, "Renamed"),
            (Tone::Success, "Checks passing"),
            (Tone::Warning, "Uncommitted changes"),
            (Tone::Danger, "Conflict"),
          ]
          .map(|(tone, label)| Dot::new(tone, label, &p).into_any_element()),
        ),
      ])
    })
    .component("Badge")
    .title("Badge, Count and Dot"),
    example!("kbd/basic", |_, _, cx| {
      let p = p(cx);
      div().flex().gap_2().children(
        ["secondary-k", "secondary-shift-p", "escape", "enter"].map(|key| Kbd::new(key, &p)),
      )
    })
    .component("Kbd")
    .title("Shortcuts"),
    example!("checkbox/states", |_, _, cx| {
      let p = p(cx);
      div().flex().items_center().gap_3().children(
        [
          ("off", CheckState::Off, false),
          ("on", CheckState::On, false),
          ("mixed", CheckState::Mixed, false),
          ("disabled", CheckState::On, true),
        ]
        .map(|(id, state, disabled)| Checkbox::new(id, &p).checked(state).disabled(disabled)),
      )
    })
    .component("Checkbox")
    .title("Checkbox")
    .size(120., 32.),
    example!("checkbox/labels", |_, _, cx| {
      let p = p(cx);
      // The label sits in the row that takes the click, so it toggles too.
      let row = |id: &'static str, state: CheckState, label: &'static str, disabled: bool| {
        div()
          .id(id)
          .flex()
          .items_center()
          .gap_2()
          .h(px(28.))
          .child(
            Checkbox::new(format!("{id}-box"), &p)
              .checked(state)
              .disabled(disabled),
          )
          .child(
            treq_ui::text(div(), treq_ui::TextSize::Sm)
              .text_color(if disabled {
                p.muted_foreground
              } else {
                p.foreground
              })
              .child(label),
          )
      };
      div()
        .w(px(300.))
        .flex()
        .flex_col()
        .child(row("all", CheckState::Mixed, "All files", false))
        .child(
          div()
            .pl_5()
            .child(row("src", CheckState::On, "src/main.rs", false))
            .child(row("lock", CheckState::Off, "Cargo.lock", false)),
        )
        .child(row("viewed", CheckState::On, "Viewed", false))
        .child(row(
          "locked",
          CheckState::On,
          "Pushed commits (locked)",
          true,
        ))
        .child(
          div()
            .id("notify")
            .flex()
            .items_start()
            .gap_2()
            .py_1()
            .child(
              div()
                .pt(px(3.))
                .child(Checkbox::new("notify-box", &p).checked(true)),
            )
            .child(
              div()
                .flex()
                .flex_col()
                .child(
                  treq_ui::text(div(), treq_ui::TextSize::Sm)
                    .text_color(p.foreground)
                    .child("Notify when the agent finishes"),
                )
                .child(
                  treq_ui::text(div(), treq_ui::TextSize::Xs)
                    .text_color(p.muted_foreground)
                    .child("A desktop notification, only while Treq is in the background."),
                ),
            ),
        )
    })
    .component("Checkbox")
    .title("With labels")
    .size(340., 230.),
    example!("alert/playground", |knobs, _, cx| {
      let p = p(cx);
      div().w(px(440.)).child(
        Alert::new("conflicts", knobs.text("title", "1 file has conflicts"), &p)
          .message("Resolve them before you push or merge.")
          .severity(knobs.pick(
            "severity",
            &[
              ("info", Severity::Info),
              ("warning", Severity::Warning),
              ("danger", Severity::Danger),
            ],
            "danger",
          ))
          .when(knobs.bool("dismiss", false), |a| {
            a.action(
              Button::new("dismiss", &p)
                .variant(Variant::Ghost)
                .size(treq_ui::Size::Xs)
                .label("Not now"),
            )
          })
          .when(knobs.bool("action", true), |a| {
            a.action(
              Button::new("resolve", &p)
                .variant(Variant::Outline)
                .size(treq_ui::Size::Xs)
                .label("Resolve…"),
            )
          }),
      )
    })
    .component("Alert")
    .title("Playground"),
    example!("empty-state/playground", |knobs, _, cx| {
      let p = p(cx);
      let error = knobs.bool("error", false);
      EmptyState::new(
        "prs",
        if error {
          "Couldn't load pull requests"
        } else {
          "No pull requests"
        },
        &p,
      )
      .icon(if error {
        theme::ICON_CIRCLE_ALERT
      } else {
        theme::ICON_CIRCLE_DOT
      })
      .error(error)
      .message(if error {
        "gh isn't signed in. Run gh auth login, then retry."
      } else {
        "Push a branch to open one."
      })
      .when(error, |e| {
        e.action(
          Button::new("retry", &p)
            .variant(Variant::Outline)
            .size(treq_ui::Size::Xs)
            .label("Retry"),
        )
      })
    })
    .component("EmptyState")
    .title("Empty and error")
    .size(420., 220.),
    example!("empty-state/loading", |_, _, cx| {
      let p = p(cx);
      div()
        .w(px(280.))
        .flex()
        .flex_col()
        .gap_4()
        .child(Loading::new("loading", &p).label("Loading commits"))
        .child(Skeleton::new("skeleton", 4, &p))
    })
    .component("EmptyState")
    .title("Loading")
    .size(320., 160.),
  ]
}

fn navigation() -> Vec<Example> {
  vec![
    example!("tabs/playground", |knobs, _, cx| {
      let p = p(cx);
      Tabs::new("views", &p)
        .style(knobs.pick(
          "style",
          &[
            ("segmented", TabsStyle::Segmented),
            ("underline", TabsStyle::Underline),
          ],
          "segmented",
        ))
        .tabs([
          Tab::new("code", "Code"),
          Tab::new("commits", "Commits").count(2),
          {
            let changes = Tab::new("changes", "Changes").count(3);
            if knobs.bool("conflict", false) {
              changes.dot(Tone::Danger, "Conflict")
            } else {
              changes
            }
          },
          {
            let logs = Tab::new("logs", "Logs");
            if knobs.bool("badge", false) {
              logs.badge("PRO", Tone::Neutral)
            } else {
              logs
            }
          },
        ])
        .selected(1)
    })
    .component("Tabs")
    .title("Playground"),
    example!("tabs/segmented", |_, _, cx| {
      let p = p(cx);
      Tabs::segmented("views", &p)
        .tab(Tab::new("code", "Code").icon(theme::ICON_CODE))
        .tab(
          Tab::new("commits", "Commits")
            .icon(theme::ICON_COMMIT)
            .count(2),
        )
        .tab(
          Tab::new("changes", "Changes")
            .icon(theme::ICON_FILE_DIFF)
            .count(3),
        )
        .tab(Tab::new("logs", "Logs").icon(theme::ICON_DATABASE))
        .selected(1)
    })
    .component("Tabs")
    .title("Segmented with icons"),
    example!("row/densities", |_, _, cx| {
      let p = p(cx);
      div()
        .w(px(360.))
        .flex()
        .flex_col()
        .child(
          Row::new("ws-1", &p)
            .icon(theme::ICON_CIRCLE_DOT)
            .title("feat/event-ingest")
            .trailing(Dot::new(Tone::GitOpen, "Pull request open", &p)),
        )
        .child(
          Row::new("ws-2", &p)
            .icon(theme::ICON_CIRCLE_DOT)
            .title("feat/keyvalues-cache")
            .selected(true)
            .trailing(Count::new(3, &p)),
        )
        .child(
          Row::new("ws-3", &p)
            .density(RowDensity::Default)
            .title("main")
            .meta("default branch"),
        )
        .child(
          Row::new("commit", &p)
            .density(RowDensity::TwoLine)
            .title("feat: keyvalues cache")
            .meta("just now · +9 −0")
            .actions(
              IconButton::new("commit-more", theme::ICON_CHEVRON_DOWN, "More", &p)
                .size(IconButtonSize::Small),
              32.,
            ),
        )
    })
    .component("Row")
    .title("Densities and selection"),
    example!("row/slots", |_, _, cx| {
      let p = p(cx);
      div()
        .w(px(360.))
        .flex()
        .flex_col()
        .child(
          Row::new("file-1", &p)
            .density(RowDensity::Default)
            .leading(Checkbox::new("file-1-check", &p).checked(CheckState::On))
            .title("src/main.rs")
            .meta("src")
            .trailing(Badge::new("M", &p).tone(Tone::Warning)),
        )
        .child(
          Row::new("file-2", &p)
            .density(RowDensity::Default)
            .leading(Checkbox::new("file-2-check", &p))
            .title("src/lib.rs")
            .meta("src")
            .selected(true)
            .trailing(Badge::new("A", &p).tone(Tone::GitOpen)),
        )
        .child(Row::load_more("files-more", false, &p))
        .child(Row::load_more("files-loading", true, &p))
    })
    .component("Row")
    .title("Slots and load more"),
    example!("menu/items", |_, _, cx| {
      let p = p(cx);
      div()
        .w(px(240.))
        .p_1()
        .rounded(px(theme::RADIUS_MD))
        .border_1()
        .border_color(p.border)
        .bg(p.popover)
        .child(MenuItem::new("copy", "Copy branch name", &p).shortcut("secondary-c"))
        .child(MenuItem::new("rename", "Rename…", &p))
        .child(
          MenuItem::new("plan", "Plan", &p)
            .description("Plan the change first, without editing files"),
        )
        .child(MenuItem::new("open-pr", "Open pull request", &p).disabled(true))
        .child(MenuSeparator::new(&p))
        .child(MenuLabel::new("Target branch", &p))
        .child(MenuItem::new("main", "main", &p).checked(true))
        .child(MenuItem::new("develop", "develop", &p).checked(false))
        .child(MenuSeparator::new(&p))
        .child(MenuItem::new("delete", "Delete workspace…", &p).danger())
    })
    .component("Menu")
    .title("Commands, values and danger"),
  ]
}

fn surfaces() -> Vec<Example> {
  vec![
    example!("dialog/confirm", |knobs, _, cx| {
      let p = p(cx);
      ConfirmDialog::new(
        "init-repo",
        "Initialize a Git repository here?",
        if knobs.bool("destructive", false) {
          "Delete workspace"
        } else {
          "Initialize"
        },
        &p,
      )
      .message("~/plain-folder isn't a Git repository yet. Treq needs one to create workspaces.")
      .when(knobs.bool("destructive", false), |d| d.destructive())
    })
    .component("Dialog")
    .title("Confirmation")
    .size(560., 280.),
    example!("dialog/close", |_, _, cx| {
      let p = p(cx);
      Dialog::new("shortcuts", &p)
        .title("Keyboard shortcuts")
        .description("No action pair, so an X closes it.")
        .on_close(|_, _| {})
        .child(
          div()
            .flex()
            .justify_between()
            .child("Open command palette")
            .child(Kbd::new("secondary-k", &p)),
        )
    })
    .component("Dialog")
    .title("Information")
    .size(560., 260.),
    example!("dialog/busy", |knobs, _, cx| {
      let p = p(cx);
      let busy = knobs.bool("busy", true);
      Dialog::new("rename", &p)
        .width(520.)
        .title("Rename workspace")
        .header(
          div()
            .flex()
            .items_center()
            .gap_2()
            .text_lg()
            .child("Rename workspace")
            .child(Badge::new("treq/login", &p)),
        )
        .primary_disabled(busy)
        .on_close(|_, _| {})
        .child("A custom header, and a verb that is busy: Enter waits for it.")
        .footer([
          Button::new("rename-cancel", &p)
            .variant(Variant::Outline)
            .label("Cancel")
            .into_any_element(),
          Button::new("rename-confirm", &p)
            .disabled(busy)
            .label(if busy { "Renaming…" } else { "Rename" })
            .into_any_element(),
        ])
    })
    .component("Dialog")
    .title("Custom header, busy verb")
    .size(600., 260.),
    example!("floating", |_, _, cx| {
      let p = p(cx);
      floating(div(), &p)
        .w(px(360.))
        .bg(p.background)
        .p_3()
        .child("A surface that floats without being a dialog: the palette, a file finder.")
    })
    .component("Floating")
    .title("Floating surface")
    .size(480., 160.),
  ]
}

fn mobile() -> Vec<Example> {
  vec![
    example!("touch/grouped-list", |_, _, cx| {
      let p = p(cx);
      div().w(px(390.)).p_4().child(
        touch::grouped(&p)
          .child(touch::nav_row("ws-1", "feat/card-system", &p).meta("2 files changed"))
          .child(touch::nav_row("ws-2", "fix/forms", &p).meta("Agent idle"))
          .child(touch::nav_row("ws-3", "main", &p).meta("Clean")),
      )
    })
    .component("Mobile")
    .title("Grouped list")
    .size(420., 260.),
    example!("touch/choice", |knobs, _, cx| {
      let p = p(cx);
      let picked = knobs.select("picked", &["merge", "squash", "rebase"], "merge");
      div().w(px(390.)).p_4().child(
        touch::grouped(&p)
          .child(touch::choice(
            "merge",
            "Merge commit",
            picked == "merge",
            &p,
          ))
          .child(touch::choice(
            "squash",
            "Squash and merge",
            picked == "squash",
            &p,
          ))
          .child(touch::choice(
            "rebase",
            "Rebase and merge",
            picked == "rebase",
            &p,
          )),
      )
    })
    .component("Mobile")
    .title("Choice list (radio)")
    .size(420., 240.),
    example!("touch/bottom-bar", |_, _, cx| {
      let p = p(cx);
      div().w(px(390.)).child(
        touch::bottom_bar(&p)
          .child(
            Button::new("test", &p)
              .variant(Variant::Outline)
              .label("Test"),
          )
          .child(
            div()
              .flex_1()
              .child(Button::new("add", &p).label("Add host").w_full()),
          ),
      )
    })
    .component("Mobile")
    .title("Bottom bar")
    .size(420., 100.),
    example!("touch/action-sheet", |_, _, cx| {
      let p = p(cx);
      div().w(px(390.)).child(touch::ActionSheet::new(
        vec![
          touch::SheetAction::new("terminal", "Open terminal", |_, _| {}),
          touch::SheetAction::new("rename", "Rename…", |_, _| {}),
          touch::SheetAction::new("copy", "Copy branch name", |_, _| {}),
          touch::SheetAction::new("delete", "Delete workspace…", |_, _| {}).danger(),
        ],
        &p,
      ))
    })
    .component("Mobile")
    .title("Action sheet (menu)")
    .size(420., 240.),
    example!("touch/tab-bar", |knobs, _, cx| {
      let p = p(cx);
      div().w(px(390.)).child(
        Tabs::new("ws-tabs", &p)
          .style(TabsStyle::Bar)
          .tab(
            Tab::new("t-term", "Terminals")
              .icon(theme::ICON_TERMINAL)
              .count(2),
          )
          .tab(
            Tab::new("t-changes", "Changes")
              .icon(theme::ICON_FILE_DIFF)
              .count(4),
          )
          .tab(Tab::new("t-commits", "Commits").icon(theme::ICON_COMMIT))
          .tab(Tab::new("t-code", "Code").icon(theme::ICON_CODE))
          .selected(knobs.number("selected", 1., 0.0..=3.0, 1.) as usize)
          .on_select(|_, _, _| {}),
      )
    })
    .component("Mobile")
    .title("Tab bar")
    .size(420., 100.),
    example!(view "input-group", |_, window, cx| {
      cx.new(|cx| InputGroupDemo {
        path: cx.new(|cx| InputState::new(window, cx).default_value("~/code/treq")),
      })
    })
    .component("InputGroup")
    .title("Path with browse and copy")
    .size(420., 100.),
    example!("touch/path-field", |_, _, cx| {
      let p = p(cx);
      div().w(px(390.)).p_4().child(
        Field::new("root", &p)
          .label("Workspace root")
          .hint("A folder on the host.")
          .child(
            PathField::new("root-path", &p)
              .value(Some("~/code"))
              .on_browse("Browse host…", |_, _| {}),
          ),
      )
    })
    .component("Mobile")
    .title("Path on a remote host")
    .size(420., 140.),
  ]
}

struct InputGroupDemo {
  path: Entity<InputState>,
}

impl Render for InputGroupDemo {
  fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
    let p = p(cx);
    div().w(px(360.)).p_4().child(
      treq_ui::InputGroup::new("demo-path", &p)
        .control(treq_ui::input(&self.path).appearance(false))
        .append(IconButton::new(
          "demo-browse",
          theme::ICON_FOLDER_SEARCH,
          "Browse host folders",
          &p,
        ))
        .append(IconButton::new(
          "demo-copy",
          theme::ICON_COPY,
          "Copy path",
          &p,
        )),
    )
  }
}
