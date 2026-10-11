-- Team entitlement and shared GitHub App installations from
-- 028_organizations_team.sql: Pro for every member of an organization with a
-- Team subscription (and for nobody else), the subscriptions view the
-- desktop app reads, attaching an installation to an organization, relink
-- protection, organization read access to installations, repositories and
-- merge queue configuration, and the webhook's organization owners.
begin;
select plan(64);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000f1', 'team-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000f2', 'team-member@example.com'),
  ('00000000-0000-0000-0000-0000000000f3', 'team-outsider@example.com'),
  ('00000000-0000-0000-0000-0000000000f4', 'team-labs-member@example.com'),
  ('00000000-0000-0000-0000-0000000000f5', 'team-second-owner@example.com');

insert into public.github_app_installations (id, account_login, account_type, app_id, linked_user_id) values
  (9501, 'acme-gh', 'Organization', 1, '00000000-0000-0000-0000-0000000000f1'),
  (9502, 'personal-gh', 'User', 1, '00000000-0000-0000-0000-0000000000f4'),
  (9503, 'other-gh', 'Organization', 1, '00000000-0000-0000-0000-0000000000f3');

insert into public.github_repositories (id, installation_id, owner, name, full_name) values
  (9601, 9501, 'acme-gh', 'api', 'acme-gh/api'),
  (9602, 9502, 'personal-gh', 'dotfiles', 'personal-gh/dotfiles'),
  (9603, 9503, 'other-gh', 'web', 'other-gh/web');

insert into public.merge_queue_configs (repo_id, target_branch, enabled) values (9601, 'main', false);

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select set_config('test.org',
  public.organization_create('00000000-0000-0000-0000-0000000000f1', 'Acme')->>'id', true);
select set_config('test.org_labs',
  public.organization_create('00000000-0000-0000-0000-0000000000f1', 'Acme Labs')->>'id', true);
select set_config('test.org_other',
  public.organization_create('00000000-0000-0000-0000-0000000000f3', 'Other')->>'id', true);
select public.organization_accept_invite('00000000-0000-0000-0000-0000000000f2',
  public.organization_invite('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org')::uuid, 'team-member@example.com')->>'token');
select public.organization_accept_invite('00000000-0000-0000-0000-0000000000f4',
  public.organization_invite('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org_labs')::uuid, 'labs@example.com')->>'token');

-- ── Before Team ───────────────────────────────────────────────────────────
select is(public.has_pro('00000000-0000-0000-0000-0000000000f2'), false,
  'membership alone grants nothing');
select is(public.organization_has_team(current_setting('test.org')::uuid), false,
  'an organization without a subscription has no Team');

-- ── Attach an installation ────────────────────────────────────────────────
select throws_ok(
  $$select public.organization_attach_installation('00000000-0000-0000-0000-0000000000f2',
    current_setting('test.org')::uuid, 9501)$$,
  'PT403', null, 'a member cannot attach an installation');
select throws_ok(
  $$select public.organization_attach_installation('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org')::uuid, 9503)$$,
  'PT404', null, 'an owner cannot attach an installation someone else linked');
select throws_ok(
  $$select public.organization_attach_installation('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org')::uuid, 424242)$$,
  'PT404', null, 'an unknown installation cannot be attached');
select is(
  public.organization_attach_installation('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org')::uuid, 9501),
  'attached', 'the owner attaches the installation they linked');
select is((select organization_id from public.github_app_installations where id = 9501),
  current_setting('test.org')::uuid, 'the installation now belongs to the organization');
select is(
  public.organization_attach_installation('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org')::uuid, 9501),
  'already_attached', 'attaching again changes nothing');
select throws_ok(
  $$select public.organization_attach_installation('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org_labs')::uuid, 9501)$$,
  'PT409', null, 'an installation cannot move to a second organization');
select is(public.installation_has_pro(9501), false,
  'an organization''s installation is not entitled until the organization has Team');

-- ── Relink protection ─────────────────────────────────────────────────────
select is(
  public.github_link_installation('00000000-0000-0000-0000-0000000000f2', 9501,
    'acme-gh', 'Organization', null, 1)->>'result',
  'organization_owner_required', 'a member cannot relink the organization''s installation');
select is(
  public.github_link_installation('00000000-0000-0000-0000-0000000000f3', 9501,
    'acme-gh', 'Organization', null, 1)->>'result',
  'organization_owner_required', 'an outsider cannot take the organization''s installation over');
select is((select linked_user_id from public.github_app_installations where id = 9501),
  '00000000-0000-0000-0000-0000000000f1'::uuid, 'the refused relinks changed nothing');
select is(
  public.github_link_installation('00000000-0000-0000-0000-0000000000f1', 9501,
    'acme-gh-renamed', 'Organization', 'https://avatars.example/acme', 1),
  jsonb_build_object('result', 'linked', 'organization_id', current_setting('test.org')),
  'an owner relinks and learns the installation''s organization');
select results_eq(
  $$select account_login, account_avatar_url, organization_id
    from public.github_app_installations where id = 9501$$,
  $$values ('acme-gh-renamed'::text, 'https://avatars.example/acme'::text,
            current_setting('test.org')::uuid)$$,
  'the relink refreshes the account and keeps the organization');
select is(
  public.github_link_installation('00000000-0000-0000-0000-0000000000f3', 9502,
    'personal-gh', 'User', null, 1)->>'result',
  'linked', 'an installation outside any organization keeps last-linker-wins');
select is((select linked_user_id from public.github_app_installations where id = 9502),
  '00000000-0000-0000-0000-0000000000f3'::uuid, 'the last linker owns the unattached installation');
select is(
  public.github_link_installation('00000000-0000-0000-0000-0000000000f1', 9504,
    'new-gh', 'Organization', null, 1),
  jsonb_build_object('result', 'linked', 'organization_id', null),
  'a new installation is linked to the caller with no organization');

-- ── Organization read access ──────────────────────────────────────────────
reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f2","role":"authenticated"}', true);
select results_eq($$select id from public.github_app_installations$$, $$values (9501::bigint)$$,
  'a member reads the organization''s installation');
select results_eq($$select id from public.github_repositories$$, $$values (9601::bigint)$$,
  'a member reads the organization''s repositories');
select results_eq($$select repo_id from public.merge_queue_configs$$, $$values (9601::bigint)$$,
  'a member reads the organization''s merge queue configuration');
select lives_ok($$update public.merge_queue_configs set batch_size = 2 where repo_id = 9601$$,
  'a member''s config update runs');
select is((select batch_size from public.merge_queue_configs where repo_id = 9601), 5,
  'but changes nothing: read access does not include writes');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f3","role":"authenticated"}', true);
select results_eq($$select id from public.github_app_installations order by id$$,
  $$values (9502::bigint), (9503::bigint)$$,
  'an outsider reads only the installations they linked');
select results_eq($$select id from public.github_repositories order by id$$,
  $$values (9602::bigint), (9603::bigint)$$,
  'an outsider reads none of the organization''s repositories');
select is((select count(*)::int from public.merge_queue_configs), 0,
  'an outsider reads none of the organization''s configs');

-- ── Team entitlement ──────────────────────────────────────────────────────
reset role;
insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
  values ('organization', current_setting('test.org')::uuid, 'cus_team_acme');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status,
   current_period_end, cancel_at_period_end, stripe_event_created)
values
  ('sub_team_acme', 'cus_team_acme', 'organization', current_setting('test.org')::uuid, 'team', 'active',
   '2026-12-01 00:00:00+00', false, now()),
  -- A Pro-plan row owned by an organization grants its members nothing.
  ('sub_org_pro', 'cus_org_pro', 'organization', current_setting('test.org_other')::uuid, 'pro', 'active',
   '2026-12-01 00:00:00+00', false, now());

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.has_pro('00000000-0000-0000-0000-0000000000f2'), true, 'a member of a Team organization has Pro');
select is(public.has_pro('00000000-0000-0000-0000-0000000000f1'), true, 'the owner has Pro');
select is(public.has_pro('00000000-0000-0000-0000-0000000000f4'), false,
  'a member of another of the owner''s organizations does not');
select is(public.has_pro('00000000-0000-0000-0000-0000000000f3'), false,
  'an organization''s Pro-plan row grants its members nothing');
select is(public.organization_has_team(current_setting('test.org')::uuid), true, 'the organization has Team');
select is(public.installation_has_pro(9501), true, 'the organization''s installation is entitled through Team');
select is(public.installation_has_pro(9504), true,
  'a member''s own installation is entitled, like any other Pro feature');
select is(public.installation_has_pro(9503), false, 'an outsider''s installation is not');

reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f2","role":"authenticated"}', true);
select results_eq(
  $$select plan, status, current_period_end from public.subscriptions$$,
  $$values ('pro'::text, 'active'::text, '2026-12-01 00:00:00'::timestamp)$$,
  'public.subscriptions reports plan pro for a Team member, so the desktop app needs no change');
select results_eq($$select stripe_subscription_id from public.billing_subscriptions$$,
  $$values ('sub_team_acme'::text)$$,
  'a member reads the organization''s subscription');
select is((select count(*)::int from public.billing_customers), 0,
  'a member cannot read the organization''s billing customer');
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f1","role":"authenticated"}', true);
select results_eq($$select owner_type, stripe_customer_id from public.billing_customers$$,
  $$values ('organization'::text, 'cus_team_acme'::text)$$,
  'the owner reads the organization''s billing customer');
select is(public.set_merge_queue_enabled('acme-gh/api', true), true,
  'the owner who linked the organization''s installation turns its merge queue on through Team');

reset role;
update public.billing_subscriptions set status = 'past_due' where stripe_subscription_id = 'sub_team_acme';
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.has_pro('00000000-0000-0000-0000-0000000000f2'), true,
  'a past_due Team keeps its members entitled while Stripe retries');

reset role;
update public.billing_subscriptions set status = 'active', cancel_at_period_end = true
  where stripe_subscription_id = 'sub_team_acme';
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f2","role":"authenticated"}', true);
select results_eq($$select plan, status from public.subscriptions$$,
  $$values ('pro'::text, 'canceled'::text)$$,
  'a Team set to cancel reads pro / canceled for its members');

reset role;
update public.billing_subscriptions set status = 'canceled' where stripe_subscription_id = 'sub_team_acme';
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.has_pro('00000000-0000-0000-0000-0000000000f2'), false, 'a canceled Team entitles nobody');
select is(public.installation_has_pro(9501), false, 'nor the organization''s installation');

reset role;
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status,
   current_period_end, cancel_at_period_end, stripe_event_created)
values
  ('sub_team_acme_2', 'cus_team_acme', 'organization', current_setting('test.org')::uuid, 'team', 'active',
   '2027-01-01 00:00:00+00', false, now());

-- ── Leaving ends Team entitlement at once (B03) ───────────────────────────
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.has_pro('00000000-0000-0000-0000-0000000000f2'), true, 'the member has Pro again');
select public.organization_remove_member('00000000-0000-0000-0000-0000000000f1',
  current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000f2');
select is(public.has_pro('00000000-0000-0000-0000-0000000000f2'), false,
  'a removed member loses Pro at once');

reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f2","role":"authenticated"}', true);
select results_eq($$select plan, status from public.subscriptions$$,
  $$values ('free'::text, 'inactive'::text)$$,
  'a removed member reads free / inactive');
select is((select count(*)::int from public.github_app_installations), 0,
  'a removed member no longer reads the organization''s installation');
select is((select count(*)::int from public.billing_subscriptions), 0,
  'a removed member no longer reads the organization''s subscription');

-- A member who comes back with a lapsed cloud workspace has the lapse
-- cleared when they join.
reset role;
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, lapsed_at)
values ('00000000-0000-0000-0000-00000000f002', '00000000-0000-0000-0000-0000000000f2',
        'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '20 days');
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select public.organization_accept_invite('00000000-0000-0000-0000-0000000000f2',
  public.organization_invite('00000000-0000-0000-0000-0000000000f1',
    current_setting('test.org')::uuid, 'team-member@example.com')->>'token');
select is((select lapsed_at from public.remote_instances where id = '00000000-0000-0000-0000-00000000f002'),
  null, 'joining a Team clears the new member''s cloud workspace lapse');

-- ── Removed owners lose the installation they linked ──────────────────────
reset role;
insert into public.organization_members (org_id, user_id, role)
  values (current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000f5', 'owner');
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(
  public.github_link_installation('00000000-0000-0000-0000-0000000000f5', 9501,
    'acme-gh', 'Organization', null, 1)->>'result',
  'linked', 'a second owner can relink the organization''s installation');
select public.organization_remove_member('00000000-0000-0000-0000-0000000000f1',
  current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000f5');
select is((select linked_user_id from public.github_app_installations where id = 9501),
  '00000000-0000-0000-0000-0000000000f1'::uuid,
  'removing the owner who linked it passes the installation to the owner who removed them');

reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000f5","role":"authenticated"}', true);
select is((select count(*)::int from public.github_app_installations where id = 9501), 0,
  'the removed owner no longer reads the installation');
select is((select count(*)::int from public.merge_queue_configs where repo_id = 9601), 0,
  'nor can they manage its merge queue configuration');

reset role;
insert into public.organization_members (org_id, user_id, role)
  values (current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000f5', 'owner');
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select public.github_link_installation('00000000-0000-0000-0000-0000000000f5', 9501,
  'acme-gh', 'Organization', null, 1);
select public.organization_leave('00000000-0000-0000-0000-0000000000f5', current_setting('test.org')::uuid);
select is((select linked_user_id from public.github_app_installations where id = 9501),
  '00000000-0000-0000-0000-0000000000f1'::uuid,
  'an owner who leaves passes the installation they linked to a remaining owner');

-- ── Webhook writes for organizations ──────────────────────────────────────
-- f4 is a member of Acme Labs with a lapsed cloud workspace.
reset role;
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, lapsed_at)
values ('00000000-0000-0000-0000-00000000f004', '00000000-0000-0000-0000-0000000000f4',
        'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '20 days');
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(
  public.billing_attach_customer('organization', current_setting('test.org_labs')::uuid, 'cus_team_labs'),
  'cus_team_labs', 'billing-checkout can record a Stripe customer for an organization');
select throws_ok(
  $$select public.billing_attach_customer('organization', '00000000-0000-0000-0000-00000000dead', 'cus_ghost_org')$$,
  'P0001', null, 'a customer cannot be recorded for an organization that does not exist');
select is(
  public.billing_record_checkout_completed('evt_team_labs_checkout', 'cus_team_labs', 'organization',
    current_setting('test.org_labs')::uuid),
  'applied', 'checkout.session.completed confirms an organization''s customer');
select is(
  public.billing_record_checkout_completed('evt_team_ghost_checkout', 'cus_team_ghost', 'organization',
    '00000000-0000-0000-0000-00000000dead'),
  'unknown_owner', 'a session for an organization that does not exist maps nothing');
select throws_ok(
  $$select public.billing_record_checkout_completed('evt_team_bad_type', 'cus_bad', 'team',
    '00000000-0000-0000-0000-0000000000f1')$$,
  '22023', null, 'any other owner type is still rejected');
select is(
  public.billing_record_subscription_event('evt_team_labs_created', 'customer.subscription.created',
    now(), 'sub_team_labs', 'cus_team_labs', 'team', 'active',
    now() + interval '30 days', false, null),
  'applied', 'a Team subscription event is recorded for the organization');
select results_eq(
  $$select owner_type, owner_id from public.billing_subscriptions where stripe_subscription_id = 'sub_team_labs'$$,
  $$values ('organization'::text, current_setting('test.org_labs')::uuid)$$,
  'the subscription belongs to the organization through its customer');
select is(public.has_pro('00000000-0000-0000-0000-0000000000f4'), true,
  'the webhook''s Team subscription entitles the organization''s members');
select is((select trial_used_at from public.billing_customers where stripe_customer_id = 'cus_team_labs'),
  null, 'Team records no trial');
select is((select lapsed_at from public.remote_instances where id = '00000000-0000-0000-0000-00000000f004'),
  null, 'a Team subscription clears its members'' cloud workspace lapses');

reset role;

select * from finish();
rollback;
