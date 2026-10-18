//! `SettingsPage`: the Repository, Application and Feature Preview tabs.
//! React sends no screen for the other tabs yet, so they stay in the
//! webview.

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, AnyElement, Context, FontWeight, SharedString, Window,
};

use super::{checklist, ActionSink, ScreenAction};
use crate::gpui_shell::{
  form::{self, Controls, SelectOption},
  theme::{self, Palette},
  ui::{self, Button, Size, TextSize, Variant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
  Repository,
  Application,
  Account,
  Integrations,
  Skills,
  Preview,
}

impl Tab {
  fn label(self) -> &'static str {
    match self {
      Tab::Repository => "Repository",
      Tab::Application => "Application",
      Tab::Account => "Account",
      Tab::Integrations => "Integrations",
      Tab::Skills => "Skills",
      Tab::Preview => "Feature Preview",
    }
  }

  fn icon(self) -> &'static str {
    match self {
      Tab::Repository => theme::ICON_FOLDER_GIT,
      Tab::Application => theme::ICON_SETTINGS,
      Tab::Account => theme::ICON_USER,
      Tab::Integrations => theme::ICON_PLUG,
      Tab::Skills => theme::ICON_BOOK_OPEN,
      Tab::Preview => theme::ICON_FLASK,
    }
  }

  fn value(self) -> &'static str {
    match self {
      Tab::Repository => "repository",
      Tab::Application => "application",
      Tab::Account => "account",
      Tab::Integrations => "integrations",
      Tab::Skills => "skills",
      Tab::Preview => "preview",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  pub tab: Tab,
  pub tabs: Vec<Tab>,
  pub save: Option<SaveButton>,
  pub application: Option<Application>,
  pub repository: Option<Repository>,
  pub preview: Option<Preview>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SaveButton {
  pub label: String,
  pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
  pub theme: String,
  pub theme_options: Vec<SelectOption>,
  pub zoom: f32,
  pub zoom_min: f32,
  pub zoom_max: f32,
  pub zoom_step: f32,
  pub font_size: String,
  pub font_size_options: Vec<SelectOption>,
  pub default_model: String,
  pub model_options: Vec<SelectOption>,
  pub default_agent: String,
  pub agent_options: Vec<SelectOption>,
  pub conflict_marker_style: String,
  pub conflict_options: Vec<SelectOption>,
  pub notify_agent_finished: bool,
  pub checklist: checklist::Checklist,
  pub setup_script: SetupScript,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupScript {
  pub script: String,
  pub always_run: bool,
  pub loading: bool,
  pub can_run: bool,
  pub status: String,
  pub status_error: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
  pub yaml_card: Option<YamlCard>,
  pub current_branch: Option<String>,
  pub loading: bool,
  pub fields: RepositoryFields,
  pub managed: Vec<String>,
  pub model_options: Vec<SelectOption>,
  pub agent_options: Vec<SelectOption>,
  pub review_agent_options: Vec<SelectOption>,
  pub trigger_options: Vec<SelectOption>,
  pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct YamlCard {
  pub error: Option<String>,
  pub loading: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFields {
  pub branch_name_pattern: String,
  pub included_files: String,
  pub default_model: String,
  pub default_agent: String,
  pub auto_push: bool,
  pub ignore_generated_agent_files: bool,
  pub review_prompt: String,
  pub review_agent: String,
  pub auto_review_trigger: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Preview {
  pub features: Vec<PreviewFeature>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PreviewFeature {
  pub id: String,
  pub title: String,
  pub checked: bool,
}

/// The settings screen and the GPUI state of its fields.
pub struct SettingsView {
  snapshot: Settings,
  dark: bool,
  sink: ActionSink,
  controls: Controls,
}

impl SettingsView {
  pub fn new(snapshot: Settings, dark: bool, sink: ActionSink) -> Self {
    Self {
      snapshot,
      dark,
      controls: Controls::new(sink.clone()),
      sink,
    }
  }

  pub fn set_snapshot(&mut self, snapshot: Settings, dark: bool, cx: &mut Context<Self>) {
    if self.snapshot != snapshot || self.dark != dark {
      self.snapshot = snapshot;
      self.dark = dark;
      cx.notify();
    }
  }

  fn press(&self, id: &str) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    let (sink, id) = (self.sink.clone(), id.to_string());
    move |_, _, cx| sink(ScreenAction::Press { id: id.clone() }, cx)
  }

  fn field(&self, field: &str) -> impl Fn(bool, &mut Window, &mut gpui::App) + 'static {
    let (sink, field) = (self.sink.clone(), field.to_string());
    move |value, _, cx| {
      sink(
        ScreenAction::Field {
          field: field.clone(),
          value: value.into(),
        },
        cx,
      )
    }
  }
}

/// `<Label>`: 13px medium, `leading-none`.
fn label(text: impl Into<SharedString>) -> gpui::Div {
  div()
    .text_size(px(13.))
    .line_height(px(13.))
    .font_weight(FontWeight::MEDIUM)
    .child(text.into())
}

/// The muted help line under a field, `mt-1` or `mt-2`.
fn help(text: impl Into<SharedString>, p: &Palette, mt: f32) -> gpui::Div {
  ui::text(div().mt(px(mt)), TextSize::Sm)
    .text_color(p.muted_foreground)
    .child(text.into())
}

/// A label above a control, with a help line under it.
fn field_block(
  name: &str,
  control: impl IntoElement,
  help_text: Option<&str>,
  p: &Palette,
) -> gpui::Div {
  div()
    .child(label(name.to_string()))
    .child(div().mt_2().child(control))
    .when_some(help_text, |this, text| {
      this.child(help(text.to_string(), p, 4.))
    })
}

/// A label and help text on the left, a switch on the right.
fn switch_row(name: &str, help_text: &str, switch: impl IntoElement, p: &Palette) -> gpui::Div {
  div()
    .flex()
    .items_center()
    .justify_between()
    .gap_4()
    .child(
      div()
        .child(label(name.to_string()))
        .child(help(help_text.to_string(), p, 4.)),
    )
    .child(switch)
}

impl Render for SettingsView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let s = self.snapshot.clone();

    let header = div()
      .flex()
      .items_center()
      .justify_between()
      .border_b_1()
      .border_color(p.border)
      .px_6()
      .py_2()
      .child(
        ui::text(div(), TextSize::Lg)
          .font_weight(FontWeight::SEMIBOLD)
          .child("Settings"),
      )
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(
            Button::new("settings-close", &p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Close")
              .on_click(self.press("settings.close")),
          )
          .when_some(s.save.clone(), |this, save| {
            this.child(
              Button::new("settings-save", &p)
                .size(Size::Sm)
                .disabled(save.disabled)
                .label(save.label)
                .on_click(self.press("settings.save")),
            )
          }),
      );

    let tabs = div()
      .w(px(192.))
      .flex_none()
      .flex()
      .flex_col()
      .gap_1()
      .children(s.tabs.iter().map(|&tab| {
        let active = tab == s.tab;
        let id = SharedString::from(format!("settings-tab-{}", tab.value()));
        let selector = id.to_string();
        let sink = self.sink.clone();
        ui::text(div(), TextSize::Sm)
          .id(id)
          .debug_selector(move || selector)
          .flex()
          .items_center()
          .gap_2()
          .px_4()
          .py_2()
          .rounded(px(8.))
          .font_weight(FontWeight::MEDIUM)
          .cursor_pointer()
          .map(|this| {
            if active {
              this.bg(p.accent).text_color(p.foreground)
            } else {
              this
                .text_color(p.muted_foreground)
                .hover(|style| style.bg(p.accent.opacity(0.5)).text_color(p.foreground))
            }
          })
          .child(ui::icon(
            tab.icon(),
            16.,
            if active {
              p.foreground
            } else {
              p.muted_foreground
            },
          ))
          .child(tab.label())
          .on_click(move |_, _, cx| {
            sink(
              ScreenAction::Field {
                field: "settings.tab".into(),
                value: tab.value().into(),
              },
              cx,
            )
          })
      }));

    let content: AnyElement = match s.tab {
      Tab::Application => match &s.application {
        Some(app) => self.application(app, &p, window, cx).into_any_element(),
        None => div().into_any_element(),
      },
      Tab::Repository => match &s.repository {
        Some(repo) => self.repository(repo, &p, window, cx).into_any_element(),
        None => div()
          .text_center()
          .py_8()
          .text_color(p.muted_foreground)
          .child("Set repository in Application tab")
          .into_any_element(),
      },
      Tab::Preview => match &s.preview {
        Some(preview) => self.preview(preview, &p).into_any_element(),
        None => div().into_any_element(),
      },
      // Not drawn by GPUI yet; React keeps the webview for these.
      Tab::Account | Tab::Integrations | Tab::Skills => div().into_any_element(),
    };

    div()
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .child(header)
      .child(
        div()
          .id("settings-scroll")
          .debug_selector(|| "settings-scroll".into())
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(
            div()
              .max_w(px(896.))
              .mx_auto()
              .p_6()
              .flex()
              .gap_8()
              .child(tabs)
              .child(div().flex_1().min_w_0().child(content)),
          ),
      )
  }
}

impl SettingsView {
  fn application(
    &mut self,
    app: &Application,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let theme_select =
      self
        .controls
        .select("app.theme", &app.theme_options, &app.theme, window, cx);
    let zoom = self.controls.slider(
      "app.zoom",
      app.zoom_min,
      app.zoom_max,
      app.zoom_step,
      app.zoom,
      window,
      cx,
    );
    let font = self.controls.select(
      "app.fontSize",
      &app.font_size_options,
      &app.font_size,
      window,
      cx,
    );
    let model = self.controls.select(
      "app.defaultModel",
      &app.model_options,
      &app.default_model,
      window,
      cx,
    );
    let agent = self.controls.select(
      "app.defaultAgent",
      &app.agent_options,
      &app.default_agent,
      window,
      cx,
    );
    let conflict = self.controls.select(
      "app.conflictMarkerStyle",
      &app.conflict_options,
      &app.conflict_marker_style,
      window,
      cx,
    );
    let script = self.controls.textarea(
      "app.setupScript",
      &app.setup_script.script,
      "brew install jj",
      4,
      window,
      cx,
    );
    let ctx = super::Ctx {
      palette: *p,
      sink: &self.sink,
    };
    let setup = &app.setup_script;

    div()
      .flex()
      .flex_col()
      .gap_6()
      .child(field_block(
        "Theme",
        form::select_field("settings-theme", &theme_select, false),
        None,
        p,
      ))
      .child(
        div()
          .child(
            div()
              .flex()
              .items_center()
              .justify_between()
              .child(label("UI Zoom"))
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child(format!("{}%", app.zoom)),
              ),
          )
          .child(div().mt_3().child(form::slider_field(&zoom)))
          .child(help(
            "Scale the application interface. Use Ctrl/Cmd + + to zoom in and Ctrl/Cmd + - to zoom out in 5% steps.",
            p,
            8.,
          )),
      )
      .child(
        div()
          .child(label("Font Size"))
          .child(
            div()
              .mt_2()
              .child(form::select_field("settings-font-size", &font, false)),
          )
          .child(help(
            "Font size for terminal, code diff display, and overview files",
            p,
            8.,
          )),
      )
      .child(field_block(
        "Claude Code Model",
        form::select_field("settings-default-model", &model, false),
        Some("Default model for new Claude Code sessions"),
        p,
      ))
      .child(field_block(
        "Default Agent",
        form::select_field("settings-default-agent", &agent, false),
        Some("Default agent for new sessions"),
        p,
      ))
      .child(checklist::render(&app.checklist, &ctx))
      .child(field_block(
        "Conflict Marker Style",
        form::select_field("settings-conflict-marker-style", &conflict, false),
        Some("Style of conflict markers written to files during rebase. Git Diff3 is compatible with most editors."),
        p,
      ))
      .child(
        div()
          .flex()
          .items_start()
          .justify_between()
          .gap_4()
          .child(
            div()
              .child(label("Notify when an agent finishes"))
              .child(help(
                "Show an OS notification when an agent finishes or asks for input while no Treq window is focused.",
                p,
                4.,
              )),
          )
          .child(div().mt_1().child(form::switch(
            "settings-notify-agent-finished",
            app.notify_agent_finished,
            false,
            p,
            self.field("app.notifyAgentFinished"),
          ))),
      )
      .child(
        div()
          .child(label("Application Setup Script"))
          .child(
            div()
              .mt_2()
              .child(form::textarea_field(&script, true, setup.loading)),
          )
          .child(help(
            "Shell script run from your home directory when you click Run. Its output is in the Logs tab under App setup.",
            p,
            4.,
          ))
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .mt_3()
              .child(form::switch(
                "settings-setup-always-run",
                setup.always_run,
                setup.loading,
                p,
                self.field("app.setupScriptAlwaysRun"),
              ))
              .child(label("Run on every app startup")),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap_3()
              .mt_3()
              .child(
                Button::new("settings-setup-run", p)
                  .variant(Variant::Outline)
                  .size(Size::Sm)
                  .disabled(!setup.can_run)
                  .child(
                    div()
                      .flex()
                      .mr_1()
                      .child(ui::icon(theme::ICON_PLAY, 12., p.foreground)),
                  )
                  .label("Run")
                  .on_click(self.press("app.setupScript.run")),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(if setup.status_error {
                    p.destructive
                  } else {
                    p.muted_foreground
                  })
                  .child(setup.status.clone()),
              ),
          ),
      )
  }

  fn repository(
    &mut self,
    repo: &Repository,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let managed = |field: &str| repo.managed.iter().any(|m| m == field);
    let f = &repo.fields;

    let yaml_card = repo.yaml_card.as_ref().map(|card| {
      let reload = Button::new("settings-yaml-reload", p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .disabled(card.loading)
        .label(if card.loading {
          "Reloading..."
        } else {
          "Reload"
        })
        .on_click(self.press("repo.yaml.reload"));
      let body = match &card.error {
        Some(error) => ui::text(div(), TextSize::Sm)
          .text_color(p.destructive)
          .child(error.clone()),
        None => ui::text(div(), TextSize::Sm)
          .child(
            div()
              .font_weight(FontWeight::MEDIUM)
              .child("Synced from .treq/config.yaml."),
          )
          .child(div().text_color(p.muted_foreground).child(
            "Settings shown as disabled below are managed by that file — edit it directly to change them.",
          )),
      };
      ui::card(p)
        .when(card.error.is_some(), |this| {
          this.border_color(p.destructive.opacity(0.5))
        })
        .when(card.error.is_none(), |this| this.bg(p.muted.opacity(0.5)))
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .p_4()
        .child(div().flex_1().child(body))
        .child(reload)
    });

    let branch = repo.current_branch.clone().map(|branch| {
      div()
        .border_b_1()
        .border_color(p.border)
        .pb_6()
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("Current Branch"),
        )
        .child(
          ui::text(div(), TextSize::Sm)
            .mt_1()
            .flex()
            .items_center()
            .gap_2()
            .child(ui::icon(theme::ICON_BRANCH, 12., p.foreground))
            .child(div().font_family(theme::MONO_FONT).child(branch)),
        )
    });

    let body: AnyElement = if repo.loading {
      div()
        .py_8()
        .text_center()
        .text_color(p.muted_foreground)
        .child("Loading settings...")
        .into_any_element()
    } else {
      let pattern = self.controls.input(
        "repo.branchNamePattern",
        &f.branch_name_pattern,
        "treq/{name}",
        window,
        cx,
      );
      let included = self.controls.textarea(
        "repo.includedFiles",
        &f.included_files,
        "e.g., .env\n.env.local",
        4,
        window,
        cx,
      );
      let model = self.controls.select(
        "repo.defaultModel",
        &repo.model_options,
        &f.default_model,
        window,
        cx,
      );
      let agent = self.controls.select(
        "repo.defaultAgent",
        &repo.agent_options,
        &f.default_agent,
        window,
        cx,
      );
      let review_prompt = self.controls.textarea(
        "repo.reviewPrompt",
        &f.review_prompt,
        "Leave empty to use the built-in review prompt",
        6,
        window,
        cx,
      );
      let review_agent = self.controls.select(
        "repo.reviewAgent",
        &repo.review_agent_options,
        &f.review_agent,
        window,
        cx,
      );
      let trigger = self.controls.select(
        "repo.autoReviewTrigger",
        &repo.trigger_options,
        &f.auto_review_trigger,
        window,
        cx,
      );
      div()
        .flex()
        .flex_col()
        .gap_6()
        .child(field_block(
          "Branch Name Pattern",
          form::input_field("settings-branch-pattern", &pattern, true, managed("branchNamePattern")),
          Some("treq/{name} → treq/add-dark-mode"),
          p,
        ))
        .child(field_block(
          "Included Files/Directories",
          form::textarea_field(&included, true, managed("includedFiles")),
          Some("Paths to copy into each new workspace (e.g. .env). For heavy dirs like node_modules, use Symlink from home repo under Advanced when creating a workspace."),
          p,
        ))
        .child(field_block(
          "Claude Code Model",
          form::select_field("settings-repo-default-model", &model, managed("defaultModel")),
          Some("Default model for new Claude Code sessions in this repository (overrides application default)"),
          p,
        ))
        .child(field_block(
          "Default Agent",
          form::select_field("settings-repo-default-agent", &agent, managed("defaultAgent")),
          Some("Default agent for new sessions in this repository (overrides application default)"),
          p,
        ))
        .child(switch_row(
          "Auto-push to remote",
          "Automatically push to the remote after every commit in this repository",
          form::switch("settings-auto-push", f.auto_push, false, p, self.field("repo.autoPush")),
          p,
        ))
        .child(switch_row(
          "Ignore generated Treq paths",
          "Add .jj*/ and Treq-generated agent skill paths to this repository's .gitignore",
          form::switch(
            "settings-ignore-generated",
            f.ignore_generated_agent_files,
            false,
            p,
            self.field("repo.ignoreGeneratedAgentFiles"),
          ),
          p,
        ))
        .child(
          div()
            .flex()
            .flex_col()
            .gap_4()
            .border_t_1()
            .border_color(p.border)
            .pt_6()
            .child(
              div()
                .child(
                  ui::text(div(), TextSize::Sm)
                    .font_weight(FontWeight::MEDIUM)
                    .child("Code Review"),
                )
                .child(help(
                  "Settings for the AI review agent. Its comments stay local to this machine and are never pushed to GitHub.",
                  p,
                  4.,
                )),
            )
            .child(field_block(
              "Custom Review Prompt",
              form::textarea_field(&review_prompt, true, managed("reviewPrompt")),
              Some("Overrides the built-in prompt. {target_type}, {target_id} and {diff_summary} are substituted before the prompt is sent."),
              p,
            ))
            .child(field_block(
              "Review Agent",
              form::select_field("settings-review-agent", &review_agent, managed("reviewAgent")),
              Some("Agent launched by Start Review (overrides the default agent)"),
              p,
            ))
            .child(field_block(
              "Automatic Review",
              form::select_field("settings-auto-review-trigger", &trigger, managed("autoReviewTrigger")),
              Some("Opens a review terminal on its own after the chosen operation finishes in a workspace of this repository."),
              p,
            )),
        )
        .when_some(repo.error.clone(), |this, error| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.destructive)
              .child(error),
          )
        })
        .into_any_element()
    };

    div()
      .flex()
      .flex_col()
      .gap_6()
      .children(yaml_card)
      .children(branch)
      .child(body)
  }

  fn preview(&self, preview: &Preview, p: &Palette) -> impl IntoElement {
    div()
      .flex()
      .flex_col()
      .gap_6()
      .child(
        div()
          .child(
            div()
              .text_size(px(13.))
              .line_height(px(20.))
              .font_weight(FontWeight::SEMIBOLD)
              .child("Feature Preview"),
          )
          .child(help(
            "Turn on experimental features. Off hides them in the UI and blocks the matching backend commands. Dev builds start with every preview flag on. Release builds use the shipped default until you change a switch.",
            p,
            4.,
          )),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_4()
          .children(preview.features.iter().map(|feature| {
            let docs = SharedString::from(format!("settings-preview-docs-{}", feature.id));
            let docs_selector = docs.to_string();
            div()
              .flex()
              .items_center()
              .justify_between()
              .gap_4()
              .rounded(px(8.))
              .border_1()
              .border_color(p.border)
              .p_4()
              .child(
                ui::text(div(), TextSize::Sm)
                  .id(docs)
                  .debug_selector(move || docs_selector)
                  .min_w_0()
                  .font_weight(FontWeight::MEDIUM)
                  .text_color(p.primary)
                  .cursor_pointer()
                  .hover(|style| style.underline())
                  .child(feature.title.clone())
                  .on_click(self.press(&format!("preview.docs:{}", feature.id))),
              )
              .child(form::switch(
                format!("settings-preview-{}", feature.id),
                feature.checked,
                false,
                p,
                self.field(&format!("preview.{}", feature.id)),
              ))
          })),
      )
  }
}
