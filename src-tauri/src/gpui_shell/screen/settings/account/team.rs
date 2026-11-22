//! The Account tab's Teams section, after the web dashboard's `TeamTab`
//! and `OrganizationCard`: each organization the user belongs to, its
//! plan, seats, members and pending invites.

use super::*;
use crate::core::teams::{Membership, TeamData, TEAM_EXTRA_SEAT_USD, TEAM_INCLUDED_SEATS};
use std::collections::BTreeMap;
use tauri_plugin_gpui::gpui::ClipboardItem;

/// What the Teams section's actions keep between draws.
#[derive(Default)]
pub(super) struct TeamActions {
  /// Each organization's invite email field.
  drafts: BTreeMap<String, String>,
  /// Accept links by invite, shown once: the server keeps only a hash.
  links: BTreeMap<String, String>,
  /// Why an organization's last action was refused.
  errors: BTreeMap<String, String>,
  pending: bool,
  /// A removal or leave waiting for the user to confirm it.
  confirm: Option<Confirm>,
}

struct Confirm {
  title: String,
  message: String,
  label: &'static str,
  org_id: String,
  body: serde_json::Value,
}

/// The Teams section, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum TeamSection {
  Loading,
  Failed(String),
  /// One card per organization; none draws nothing.
  Loaded(Vec<OrgCard>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrgCard {
  pub id: String,
  pub name: String,
  /// The user's role: `Owner`, `Admin` or `Member`.
  pub role: &'static str,
  /// `Team · Active`, or `No Team subscription`.
  pub plan: String,
  /// What the period end means, and the date.
  pub renews: Option<(String, String)>,
  /// Seats for owners and admins, who pay for them; the count for members.
  pub seats: String,
  pub members: Vec<MemberRow>,
  /// Pending invites, which only owners and admins see.
  pub invites: Option<Vec<InviteRow>>,
  /// The invite email field, for owners and admins.
  pub invite_draft: Option<String>,
  /// An action on any team runs.
  pub pending: bool,
  /// Why this team's last action was refused.
  pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberRow {
  pub user_id: String,
  /// The member's name (or email), with ` (you)` for the user.
  pub name: String,
  pub email: Option<String>,
  pub role: &'static str,
  /// "Make admin" or "Remove admin", for owners and admins on others.
  pub admin_toggle: Option<&'static str>,
  /// Owners remove anyone but themselves.
  pub removable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InviteRow {
  pub id: String,
  pub email: String,
  /// `Expires Nov 16, 2026`.
  pub expires: String,
  /// The accept link, once generated here.
  pub link: Option<String>,
}

/// `roleLabel`.
fn role_label(role: Option<&str>) -> &'static str {
  match role {
    Some("owner") => "Owner",
    Some("admin") => "Admin",
    _ => "Member",
  }
}

/// `shortDate`: `Nov 16, 2026`, in UTC.
fn short_date(at: &str) -> String {
  chrono::DateTime::parse_from_rfc3339(at)
    .map(|at| {
      at.with_timezone(&chrono::Utc)
        .format("%b %-d, %Y")
        .to_string()
    })
    .unwrap_or_else(|_| at.to_string())
}

/// The section for what the account read: `None` while it reads.
pub(super) fn section(
  team: Option<&Result<TeamData, String>>,
  actions: &TeamActions,
) -> TeamSection {
  match team {
    None => TeamSection::Loading,
    Some(Err(_)) => {
      TeamSection::Failed("Couldn't load your teams. Reopen Settings to try again.".into())
    }
    Some(Ok(data)) => TeamSection::Loaded(org_cards(data, actions)),
  }
}

/// The Team plan's features, as the pricing page lists them (`TEAM_PERKS`).
const TEAM_PERKS: [&str; 4] = [
  "Everything in Pro for every member",
  "Manage team-wide integrations",
  "Sandbox usage management",
  "One bill for the whole team",
];

/// Whether to offer Team (`GetTeamCard`): the teams are read and none the
/// user belongs to has a Team subscription.
pub(super) fn upsell(team: Option<&Result<TeamData, String>>) -> bool {
  let Some(Ok(data)) = team else {
    return false;
  };
  !data
    .memberships
    .iter()
    .any(|m| m.user_id == data.user_id && data.subscriptions.iter().any(|s| s.owner_id == m.org_id))
}

fn org_cards(data: &TeamData, actions: &TeamActions) -> Vec<OrgCard> {
  data
    .memberships
    .iter()
    .filter(|m| m.user_id == data.user_id)
    .filter_map(|m| m.organizations.clone())
    .map(|org| {
      let members: Vec<&Membership> = data
        .memberships
        .iter()
        .filter(|m| m.org_id == org.id)
        .collect();
      let mine = members
        .iter()
        .find(|m| m.user_id == data.user_id)
        .map(|m| m.role.as_str());
      let manager = matches!(mine, Some("owner" | "admin"));
      let owner = mine == Some("owner");
      let subscription = data.subscriptions.iter().find(|s| s.owner_id == org.id);
      let count = members.len();
      let extra = count.saturating_sub(TEAM_INCLUDED_SEATS);
      OrgCard {
        role: role_label(mine),
        plan: subscription.map_or_else(
          || "No Team subscription".to_string(),
          |s| {
            let status = if s.cancel_at_period_end {
              "canceled"
            } else {
              &s.status
            };
            format!("Team · {}", status_label(status))
          },
        ),
        renews: subscription.and_then(|s| {
          let label = if s.cancel_at_period_end {
            "Team until"
          } else {
            "Renews on"
          };
          Some((label.to_string(), local_date(s.current_period_end.as_ref()?)))
        }),
        seats: if !manager {
          plural(count as u64, "member", "members")
        } else if extra > 0 {
          format!(
            "{count} of {TEAM_INCLUDED_SEATS} included, {extra} extra at US${TEAM_EXTRA_SEAT_USD} per month each"
          )
        } else {
          format!("{count} of {TEAM_INCLUDED_SEATS} included")
        },
        members: members
          .iter()
          .map(|m| {
            let profile = data.profiles.iter().find(|p| p.id == m.user_id);
            let email = profile.and_then(|p| p.email.clone());
            let name = profile
              .and_then(|p| p.full_name.clone().filter(|n| !n.is_empty()))
              .or_else(|| email.clone())
              .unwrap_or_else(|| "Unknown member".into());
            let me = m.user_id == data.user_id;
            MemberRow {
              user_id: m.user_id.clone(),
              admin_toggle: (manager && !me && m.role != "owner").then_some(
                if m.role == "admin" {
                  "Remove admin"
                } else {
                  "Make admin"
                },
              ),
              removable: owner && !me,
              name: if me {
                format!("{name} (you)")
              } else {
                name
              },
              email,
              role: role_label(Some(&m.role)),
            }
          })
          .collect(),
        invites: manager.then(|| {
          data
            .invites
            .iter()
            .filter(|i| i.org_id == org.id)
            .map(|i| InviteRow {
              id: i.id.clone(),
              email: i.email.clone(),
              expires: format!("Expires {}", short_date(&i.expires_at)),
              link: actions.links.get(&i.id).cloned(),
            })
            .collect()
        }),
        invite_draft: manager.then(|| actions.drafts.get(&org.id).cloned().unwrap_or_default()),
        pending: actions.pending,
        error: actions.errors.get(&org.id).cloned(),
        id: org.id,
        name: org.name,
      }
    })
    .collect()
}

impl SettingsView {
  /// Reads the user's organizations, once per sign-in.
  pub(super) fn load_team(&mut self, cx: &mut Context<Self>) {
    self.account.team_loading = true;
    self.spawn(
      cx,
      |host| host.team(),
      |view, result, _| {
        view.account.team_loading = false;
        view.account.team = Some(result.map_err(|error| {
          log::warn!("reading the teams failed: {error}");
          error.to_string()
        }));
      },
    );
  }

  /// "Get Team": the plan's price and perks. The website creates the team
  /// and takes the payment, so Start a team opens the dashboard.
  pub(super) fn team_upsell(&self, p: &Palette) -> AnyElement {
    let included =
      format!("{TEAM_INCLUDED_SEATS} members included, US${TEAM_EXTRA_SEAT_USD} per extra member");
    ui::text(div(), TextSize::Sm)
      .selector("team-upsell")
      .role(gpui::Role::Group)
      .aria_label("Team plan")
      .flex()
      .flex_col()
      .gap_2()
      .border_1()
      .border_color(p.border)
      .rounded(radius::MD)
      .p_4()
      .child(div().font_weight(FontWeight::MEDIUM).child("Team plan"))
      .child(
        div()
          .flex()
          .items_baseline()
          .gap_1()
          .child(
            ui::text(div(), TextSize::Lg)
              .font_weight(FontWeight::SEMIBOLD)
              .child("US$199"),
          )
          .child(div().text_color(p.muted_foreground).child("per month")),
      )
      .child(div().text_color(p.muted_foreground).child(included))
      .children(TEAM_PERKS.iter().map(|perk| {
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_CHECK, 14., Tone::Success.color(p)))
          .child(*perk)
      }))
      .child(
        ui::card_actions()
          .mt_2()
          .child(
            Button::new("team-compare", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .label("View full comparison")
              .on_click(self.press("team.compare")),
          )
          .child(
            Button::new("team-start", p)
              .size(Size::Sm)
              .child(ui::icon(
                theme::ICON_EXTERNAL_LINK,
                12.,
                p.primary_foreground,
              ))
              .label("Start a team")
              .on_click(self.press("team.start")),
          ),
      )
      .test_support()
      .into_any_element()
  }

  /// A Teams button press (`teams.*`).
  pub(super) fn press_team(&mut self, id: &str, cx: &mut Context<Self>) {
    let Some(Ok(data)) = &self.account.team else {
      return;
    };
    let (verb, arg) = id
      .strip_prefix("teams.")
      .and_then(|rest| rest.split_once('.'))
      .unwrap_or((id, ""));
    let (org, user) = arg.split_once('.').unwrap_or((arg, ""));
    let name = data
      .profiles
      .iter()
      .find(|p| p.id == user)
      .and_then(|p| p.full_name.clone().or_else(|| p.email.clone()))
      .unwrap_or_else(|| "this member".into());
    let org_name = data
      .memberships
      .iter()
      .find(|m| m.org_id == org)
      .and_then(|m| m.organizations.as_ref())
      .map_or_else(|| "this team".into(), |o| o.name.clone());
    let invite = data.invites.iter().find(|i| i.id == arg).cloned();
    let admin = data
      .memberships
      .iter()
      .any(|m| m.org_id == org && m.user_id == user && m.role == "admin");
    let actions = &mut self.account.team_actions;
    match (verb, id) {
      (_, "teams.confirm") => {
        if let Some(confirm) = actions.confirm.take() {
          self.run_team(confirm.org_id, confirm.body, |_, _| {}, cx);
        }
      }
      (_, "teams.confirm.cancel") => actions.confirm = None,
      ("invite", _) => {
        let email = actions.drafts.get(org).map(|e| e.trim().to_string());
        if let Some(email) = email.filter(|e| !e.is_empty()) {
          self.send_invite(org.to_string(), email, cx);
        }
      }
      ("renew", _) => {
        if let Some(i) = invite {
          self.send_invite(i.org_id, i.email, cx);
        }
      }
      ("copy", _) => {
        if let Some(link) = actions.links.get(arg) {
          cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
        }
      }
      ("revoke", _) => {
        if let Some(i) = invite {
          let body = serde_json::json!({ "action": "revoke_invite", "invite_id": i.id });
          self.run_team(i.org_id, body, |_, _| {}, cx);
        }
      }
      ("role", _) => {
        let body = serde_json::json!({
          "action": "set_role", "organization_id": org, "user_id": user,
          "role": if admin { "member" } else { "admin" },
        });
        self.run_team(org.to_string(), body, |_, _| {}, cx);
      }
      ("remove", _) => {
        actions.confirm = Some(Confirm {
          title: format!("Remove {name}?"),
          message: format!("They leave {org_name} and lose its Team at once."),
          label: "Remove member",
          org_id: org.to_string(),
          body: serde_json::json!({
            "action": "remove_member", "organization_id": org, "user_id": user,
          }),
        });
      }
      ("leave", _) => {
        actions.confirm = Some(Confirm {
          title: format!("Leave {org_name}?"),
          message: "You lose its Team at once.".into(),
          label: "Leave team",
          org_id: org.to_string(),
          body: serde_json::json!({ "action": "leave", "organization_id": org }),
        });
      }
      _ => {}
    }
  }

  /// The invite email field, `teams.inviteEmail.<org>`.
  pub(super) fn set_team_field(&mut self, field: &str, text: String) -> bool {
    let Some(org) = field.strip_prefix("teams.inviteEmail.") else {
      return false;
    };
    self
      .account
      .team_actions
      .drafts
      .insert(org.to_string(), text);
    true
  }

  fn send_invite(&mut self, org: String, email: String, cx: &mut Context<Self>) {
    let body = serde_json::json!({ "action": "invite", "organization_id": org, "email": email });
    let done_org = org.clone();
    self.run_team(
      org,
      body,
      move |actions, reply| {
        if let (Some(id), Some(url)) =
          (reply["invite"]["id"].as_str(), reply["accept_url"].as_str())
        {
          actions.links.insert(id.to_string(), url.to_string());
        }
        actions.drafts.remove(&done_org);
      },
      cx,
    );
  }

  /// Sends one `organizations` action for `org`, then reads the teams
  /// again; a refusal shows on that team's card.
  fn run_team(
    &mut self,
    org: String,
    body: serde_json::Value,
    done: impl FnOnce(&mut TeamActions, &serde_json::Value) + 'static,
    cx: &mut Context<Self>,
  ) {
    let actions = &mut self.account.team_actions;
    if actions.pending {
      return;
    }
    actions.pending = true;
    actions.errors.remove(&org);
    self.spawn(
      cx,
      move |host| host.organization_action(&body),
      move |view, result, cx| {
        let actions = &mut view.account.team_actions;
        actions.pending = false;
        match result {
          Ok(reply) => {
            done(actions, &reply);
            view.load_team(cx);
          }
          Err(error) => {
            actions.errors.insert(org, error.message);
          }
        }
      },
    );
  }

  /// The removal or leave confirmation, while one waits.
  pub(in crate::gpui_shell::screen) fn team_confirm_dialog(
    &self,
    p: &Palette,
  ) -> Option<AnyElement> {
    let confirm = self.account.team_actions.confirm.as_ref()?;
    Some(
      div()
        .absolute()
        .inset_0()
        .child(
          ui::ConfirmDialog::new(
            "teams-confirm-dialog",
            confirm.title.clone(),
            confirm.label,
            p,
          )
          .message(confirm.message.clone())
          .destructive()
          .on_cancel(handler(Box::new(self.press("teams.confirm.cancel"))))
          .on_confirm(handler(Box::new(self.press("teams.confirm")))),
        )
        .into_any_element(),
    )
  }

  /// "Teams": one card per organization, or nothing without one.
  pub(super) fn teams(
    &mut self,
    section: &TeamSection,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let orgs = match section {
      TeamSection::Loading => {
        return Some(
          ui::Loading::new("team-loading", p)
            .label("Loading teams…")
            .into_any_element(),
        )
      }
      TeamSection::Failed(error) => {
        return Some(
          ui::text(div(), TextSize::Sm)
            .text_color(Tone::Danger.color(p))
            .child(error.clone())
            .into_any_element(),
        )
      }
      TeamSection::Loaded(orgs) if orgs.is_empty() => return None,
      TeamSection::Loaded(orgs) => orgs,
    };
    let row = |label: String, value: String| {
      div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(div().text_color(p.muted_foreground).child(label))
        .child(div().child(value))
    };
    let subheading = |text: &'static str| {
      div()
        .mt_4()
        .mb_1()
        .font_weight(FontWeight::MEDIUM)
        .child(text)
    };
    let person = |name: String, detail: Option<String>, trailing: Vec<AnyElement>| {
      div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1()
        .child(
          div()
            .min_w_0()
            .flex()
            .gap_2()
            .child(div().truncate().child(name))
            .children(detail.map(|d| div().truncate().text_color(p.muted_foreground).child(d))),
        )
        .child(
          div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .children(trailing),
        )
    };
    let muted = |text: String| {
      div()
        .text_color(p.muted_foreground)
        .child(text)
        .into_any_element()
    };
    let inputs: Vec<_> = orgs
      .iter()
      .map(|org| {
        let field = format!("teams.inviteEmail.{}", org.id);
        let draft = org.invite_draft.as_ref()?;
        Some(
          self
            .controls
            .input(&field, draft, "teammate@example.com", window, cx),
        )
      })
      .collect();
    let cards = orgs.iter().zip(inputs).map(|(org, input)| {
      let id = org.id.clone();
      let pending = org.pending;
      let link_button = |id: String, label: &'static str, press: &str, pending: bool| {
        Button::new(id, p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .disabled(pending)
          .label(label)
          .on_click(self.press(press))
          .into_any_element()
      };
      let members: Vec<_> = org
        .members
        .iter()
        .map(|m| {
          let user = &m.user_id;
          let mut trailing = vec![muted(m.role.into())];
          if let Some(label) = m.admin_toggle {
            let press = format!("teams.role.{id}.{user}");
            trailing.push(link_button(
              format!("teams-role-{user}"),
              label,
              &press,
              pending,
            ));
          }
          if m.removable {
            let press = format!("teams.remove.{id}.{user}");
            trailing.push(link_button(
              format!("teams-remove-{user}"),
              "Remove",
              &press,
              pending,
            ));
          }
          person(m.name.clone(), m.email.clone(), trailing)
        })
        .collect();
      let invites = org.invites.as_ref().map(|invites| {
        let rows: Vec<AnyElement> = if invites.is_empty() {
          vec![muted("No pending invites.".into())]
        } else {
          invites
            .iter()
            .map(|invite| {
              let id = invite.id.clone();
              let link = if invite.link.is_some() {
                let press = format!("teams.copy.{id}");
                link_button(format!("teams-copy-{id}"), "Copy link", &press, false)
              } else {
                let press = format!("teams.renew.{id}");
                link_button(format!("teams-renew-{id}"), "New link", &press, pending)
              };
              let revoke = Button::new(format!("teams-revoke-{id}"), p)
                .aria_label(format!("Revoke invite for {}", invite.email))
                .variant(Variant::Ghost)
                .size(Size::Icon)
                .disabled(pending)
                .child(ui::icon(theme::ICON_TRASH, 14., p.muted_foreground))
                .on_click(self.press(&format!("teams.revoke.{id}")));
              let trailing = vec![
                muted(invite.expires.clone()),
                link,
                revoke.into_any_element(),
              ];
              div()
                .id(SharedString::from(format!("team-invite-row-{id}")))
                .role(gpui::Role::ListItem)
                .aria_label(invite.email.clone())
                .child(
                  person(invite.email.clone(), None, trailing)
                    .debug_selector(move || format!("team-invite-{id}")),
                )
                .children(invite.link.clone().map(|link| {
                  div()
                    .truncate()
                    .font_family(theme::MONO_FONT)
                    .text_color(p.muted_foreground)
                    .child(link)
                }))
                .test_support()
                .into_any_element()
            })
            .collect()
        };
        div().child(subheading("Pending invites")).children(rows)
      });
      let invite_form = input.map(|input| {
        let blank = org
          .invite_draft
          .as_deref()
          .is_none_or(|d| d.trim().is_empty());
        div()
          .mt_2()
          .flex()
          .gap_2()
          .child(div().flex_1().child(form::input_field(
            format!("teams-invite-email-{id}"),
            &input,
            false,
            pending,
          )))
          .child(
            Button::new(format!("teams-invite-{id}"), p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .disabled(pending || blank)
              .label("Generate invite link")
              .on_click(self.press(&format!("teams.invite.{id}"))),
          )
      });
      let selector = format!("team-org-{id}");
      ui::text(div(), TextSize::Sm)
        .selector(SharedString::from(selector))
        .role(gpui::Role::Group)
        .aria_label(org.name.clone())
        .border_1()
        .border_color(p.border)
        .rounded(radius::MD)
        .p_4()
        .child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .mb_3()
            .child(ui::icon(theme::ICON_USER, 16., p.foreground))
            .child(
              div()
                .font_weight(FontWeight::MEDIUM)
                .child(org.name.clone()),
            ),
        )
        .child(
          div()
            .flex()
            .flex_col()
            .gap_2()
            .child(row("Your role".into(), org.role.into()))
            .child(row("Plan".into(), org.plan.clone()))
            .children(org.renews.clone().map(|(label, date)| row(label, date)))
            .child(row(
              if org.invites.is_some() {
                "Seats"
              } else {
                "Members"
              }
              .into(),
              org.seats.clone(),
            )),
        )
        .child(subheading("Members"))
        .children(members)
        .children(invites)
        .children(invite_form)
        .when_some(org.error.clone(), |this, error| {
          this.child(div().mt_2().text_color(Tone::Danger.color(p)).child(error))
        })
        .child(
          div().mt_4().flex().child(
            Button::new(format!("teams-leave-{id}"), p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .text_color(p.destructive)
              .disabled(pending)
              .label("Leave team")
              .on_click(self.press(&format!("teams.leave.{id}"))),
          ),
        )
        .test_support()
        .into_any_element()
    });
    Some(
      div()
        .flex()
        .flex_col()
        .gap_3()
        .child(heading("Teams"))
        .children(cards)
        .into_any_element(),
    )
  }
}
