//! The Account tab's Teams section, after the web dashboard's `TeamTab`
//! and `OrganizationCard`: each organization the user belongs to, its
//! plan, seats, members and pending invites.

use super::*;
use crate::core::teams::{Membership, TeamData, TEAM_EXTRA_SEAT_USD, TEAM_INCLUDED_SEATS};

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberRow {
  /// The member's name (or email), with ` (you)` for the user.
  pub name: String,
  pub email: Option<String>,
  pub role: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InviteRow {
  pub id: String,
  pub email: String,
  /// `Expires Nov 16, 2026`.
  pub expires: String,
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
pub(super) fn section(team: Option<&Result<TeamData, String>>) -> TeamSection {
  match team {
    None => TeamSection::Loading,
    Some(Err(_)) => {
      TeamSection::Failed("Couldn't load your teams. Reopen Settings to try again.".into())
    }
    Some(Ok(data)) => TeamSection::Loaded(org_cards(data)),
  }
}

fn org_cards(data: &TeamData) -> Vec<OrgCard> {
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
            MemberRow {
              name: if m.user_id == data.user_id {
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
            })
            .collect()
        }),
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

  /// "Teams": one card per organization, or nothing without one.
  pub(super) fn teams(section: &TeamSection, p: &Palette) -> Option<AnyElement> {
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
    let person = |name: String, detail: Option<String>, trailing: String| {
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
            .text_color(p.muted_foreground)
            .child(trailing),
        )
    };
    let cards = orgs.iter().map(|org| {
      let id = org.id.clone();
      let invites = org.invites.as_ref().map(|invites| {
        let rows: Vec<AnyElement> = if invites.is_empty() {
          vec![div()
            .text_color(p.muted_foreground)
            .child("No pending invites.")
            .into_any_element()]
        } else {
          invites
            .iter()
            .map(|invite| {
              let id = invite.id.clone();
              person(invite.email.clone(), None, invite.expires.clone())
                .debug_selector(move || format!("team-invite-{id}"))
                .into_any_element()
            })
            .collect()
        };
        div().child(subheading("Pending invites")).children(rows)
      });
      ui::text(div(), TextSize::Sm)
        .debug_selector(move || format!("team-org-{id}"))
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
        .children(
          org
            .members
            .iter()
            .map(|m| person(m.name.clone(), m.email.clone(), m.role.into())),
        )
        .children(invites)
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
