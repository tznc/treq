//! The Account tab's Teams section (the web dashboard's `TeamTab`).

use gpui::TestAppContext;

use super::*;
use crate::core::{supabase::ApiError, teams::TeamData};
use settings::{InviteRow, MemberRow, OrgCard, TeamSection};

/// Ada in Acme as `role`, with Bob, a pending invite for Cy, and Team.
fn acme(role: &str) -> TeamData {
  let membership = |user: &str, role: &str| {
    serde_json::from_value(serde_json::json!({
      "org_id": "o1", "user_id": user, "role": role,
      "organizations": { "id": "o1", "name": "Acme" },
    }))
    .unwrap()
  };
  TeamData {
    user_id: "u1".into(),
    memberships: vec![membership("u1", role), membership("u2", "admin")],
    profiles: serde_json::from_value(serde_json::json!([
      { "id": "u1", "email": "ada@example.com", "full_name": "Ada" },
      { "id": "u2", "email": "bob@example.com", "full_name": null },
    ]))
    .unwrap(),
    invites: serde_json::from_value(serde_json::json!([
      { "id": "i1", "org_id": "o1", "email": "cy@example.com", "expires_at": "2026-11-16T12:00:00Z" },
    ]))
    .unwrap(),
    subscriptions: serde_json::from_value(serde_json::json!([
      { "owner_id": "o1", "status": "active", "current_period_end": null, "cancel_at_period_end": false },
    ]))
    .unwrap(),
  }
}

fn team_section(
  cx: &mut TestAppContext,
  team: Result<TeamData, ApiError>,
) -> (Harness, TeamSection) {
  let host = Arc::new(FakeHost {
    team: Some(team),
    ..FakeHost::default()
  });
  let mut h = signed_in_as_ada(cx, host);
  let section = match drawn_settings(&mut h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => signed_in.team.expect("team section"),
    other => panic!("signed in: {other:?}"),
  };
  (h, section)
}

#[gpui::test]
fn an_owner_sees_the_team_its_members_seats_and_invites(cx: &mut TestAppContext) {
  let (mut h, section) = team_section(cx, Ok(acme("owner")));
  assert_eq!(
    section,
    TeamSection::Loaded(vec![OrgCard {
      id: "o1".into(),
      name: "Acme".into(),
      role: "Owner",
      plan: "Team · Active".into(),
      renews: None,
      seats: "2 of 5 included".into(),
      members: vec![
        MemberRow {
          name: "Ada (you)".into(),
          email: Some("ada@example.com".into()),
          role: "Owner",
        },
        MemberRow {
          name: "bob@example.com".into(),
          email: Some("bob@example.com".into()),
          role: "Admin",
        },
      ],
      invites: Some(vec![InviteRow {
        id: "i1".into(),
        email: "cy@example.com".into(),
        expires: "Expires Nov 16, 2026".into(),
      }]),
    }])
  );
  assert!(h.drawn("team-org-o1"));
  assert!(h.drawn("team-invite-i1"));
}

#[gpui::test]
fn a_member_sees_the_member_count_and_no_invites(cx: &mut TestAppContext) {
  let (_, section) = team_section(cx, Ok(acme("member")));
  let TeamSection::Loaded(orgs) = section else {
    panic!("loaded: {section:?}");
  };
  assert_eq!(orgs[0].role, "Member");
  assert_eq!(orgs[0].seats, "2 members");
  assert_eq!(orgs[0].invites, None);
}

#[gpui::test]
fn extra_seats_are_priced(cx: &mut TestAppContext) {
  let mut team = acme("owner");
  for n in 3..=7 {
    let mut extra = team.memberships[1].clone();
    extra.user_id = format!("u{n}");
    team.memberships.push(extra);
  }
  let (_, section) = team_section(cx, Ok(team));
  let TeamSection::Loaded(orgs) = section else {
    panic!("loaded: {section:?}");
  };
  assert_eq!(
    orgs[0].seats,
    "7 of 5 included, 2 extra at US$8 per month each"
  );
}

#[gpui::test]
fn a_team_that_fails_to_load_says_so(cx: &mut TestAppContext) {
  let (_, section) = team_section(cx, Err(ApiError::new("network", "offline")));
  assert_eq!(
    section,
    TeamSection::Failed("Couldn't load your teams. Reopen Settings to try again.".into())
  );
}

fn signed_in(h: &mut Harness) -> settings::SignedIn {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => *signed_in,
    other => panic!("signed in: {other:?}"),
  }
}

/// Without a team that has Team, the page offers it (`GetTeamCard`); the
/// website creates the team and takes the payment.
#[gpui::test]
fn a_user_without_team_is_offered_it(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_as_ada(cx, host.clone());
  assert!(signed_in(&mut h).team_upsell);
  h.click("team-start");
  h.click("team-compare");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "open http://localhost:3001/dashboard",
      "open http://localhost:3001/pricing#feature-comparison",
    ]
  );
}

#[gpui::test]
fn a_member_of_a_team_with_team_is_not_offered_it(cx: &mut TestAppContext) {
  let (mut h, _) = team_section(cx, Ok(acme("member")));
  assert!(!signed_in(&mut h).team_upsell);
  assert!(!h.drawn("team-upsell"));
}

#[gpui::test]
fn a_team_without_a_subscription_still_gets_the_offer(cx: &mut TestAppContext) {
  let mut team = acme("owner");
  team.subscriptions.clear();
  let (mut h, _) = team_section(cx, Ok(team));
  assert!(signed_in(&mut h).team_upsell);
}

/// `SubscriptionTab`'s notes for a failed payment and a cancellation.
#[gpui::test]
fn the_subscription_explains_a_failed_payment_and_a_cancellation(cx: &mut TestAppContext) {
  let mut h = signed_in_as_ada(cx, Arc::new(FakeHost::default()));
  assert_eq!(signed_in(&mut h).billing_note, None);
  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "subscription": { "plan": "pro", "status": "past_due", "currentPeriodEnd": "2026-11-01T12:00:00Z" },
  })));
  assert_eq!(
    signed_in(&mut h).billing_note.as_deref(),
    Some("Your last payment failed. Update your payment method to keep Pro.")
  );
  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "subscription": { "plan": "pro", "status": "canceled", "currentPeriodEnd": "2026-11-01T12:00:00Z" },
  })));
  assert_eq!(
    signed_in(&mut h).billing_note.as_deref(),
    Some("Your subscription is set to cancel. Pro stays on until 11/1/2026.")
  );
}
