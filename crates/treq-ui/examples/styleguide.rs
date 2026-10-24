//! Treq's design system, built with gpui-styleguidist.
//!
//! - `cargo run -p treq-ui --example styleguide` opens it.
//! - `-- render` re-renders the snapshots under `styleguide/`; `-- check`
//!   compares against them.

use gpui::{div, prelude::*, px, App};
use gpui_styleguidist::{example, Example, Styleguide};
use treq_ui::{
  theme::{self, Palette},
  Alert, Badge, Button, CheckState, Checkbox, ConfirmDialog, Count, Dialog, Dot, EmptyState,
  IconButton, IconButtonSize, Kbd, Loading, MenuItem, MenuLabel, MenuSeparator, Picker,
  PickerPlacement, Row, RowDensity, Severity, Skeleton, SplitButton, Tab, Tabs, TabsStyle, Tone,
  Variant,
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
    .themes(["light", "dark"], |name, cx| {
      theme::set_dark(name == "dark", cx)
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
      "EmptyState",
      "Says why it's empty and what to do, with at most one action. Loading shows where the \
       result will appear.",
    )
    .examples(buttons())
    .examples(indicators())
    .examples(navigation())
    .examples(surfaces())
}

fn variant_knob(knobs: &gpui_styleguidist::Knobs) -> Variant {
  knobs.pick(
    "variant",
    &[
      ("default", Variant::Default),
      ("outline", Variant::Outline),
      ("ghost", Variant::Ghost),
      ("destructive", Variant::Destructive),
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
    })
    .component("IconButton")
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
          [
            Tone::Neutral,
            Tone::Primary,
            Tone::Success,
            Tone::Warning,
            Tone::Danger,
          ]
          .map(|tone| Dot::new(tone, &p).into_any_element()),
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
              changes.dot(Tone::Danger)
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
            .trailing(Dot::new(Tone::GitOpen, &p)),
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
            .density(RowDensity::Comfortable)
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
  ]
}
