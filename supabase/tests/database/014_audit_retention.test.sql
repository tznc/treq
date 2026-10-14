-- Audit event retention from 031_retention_windows.sql: Team members keep
-- remote audit events for 7 days, everyone else for 1 day.
begin;
select plan(7);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000c1', 'retention-team-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000c2', 'retention-pro@example.com'),
  ('00000000-0000-0000-0000-0000000000c3', 'retention-lapsed-org@example.com');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select set_config('test.org_team',
  public.organization_create('00000000-0000-0000-0000-0000000000c1', 'Retention Team')->>'id', true);
select set_config('test.org_free',
  public.organization_create('00000000-0000-0000-0000-0000000000c3', 'Retention Free')->>'id', true);
reset role;

insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_ret_team', 'cus_ret_team', 'organization', current_setting('test.org_team')::uuid, 'team', 'active', now()),
  ('sub_ret_pro', 'cus_ret_pro', 'user', '00000000-0000-0000-0000-0000000000c2', 'pro', 'active', now());

insert into public.remote_audit_events (owner_user_id, event_type, created_at) values
  ('00000000-0000-0000-0000-0000000000c1', 'team_fresh', now() - interval '12 hours'),
  ('00000000-0000-0000-0000-0000000000c1', 'team_3d', now() - interval '3 days'),
  ('00000000-0000-0000-0000-0000000000c1', 'team_8d', now() - interval '8 days'),
  ('00000000-0000-0000-0000-0000000000c2', 'pro_fresh', now() - interval '12 hours'),
  ('00000000-0000-0000-0000-0000000000c2', 'pro_2d', now() - interval '2 days'),
  ('00000000-0000-0000-0000-0000000000c3', 'org_without_team_2d', now() - interval '2 days');

select is(
  (select retention_days from public.remote_audit_retention_config),
  1, 'the default window is 1 day');
select is(
  (select team_retention_days from public.remote_audit_retention_config),
  7, 'the Team window is 7 days');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.prune_remote_audit_events(), 3::bigint, 'prune deletes the three expired events');
reset role;

select results_eq(
  $$select event_type from public.remote_audit_events
    where owner_user_id = '00000000-0000-0000-0000-0000000000c1' order by event_type$$,
  $$values ('team_3d'), ('team_fresh')$$,
  'a Team member keeps events younger than 7 days');
select results_eq(
  $$select event_type from public.remote_audit_events
    where owner_user_id = '00000000-0000-0000-0000-0000000000c2'$$,
  $$values ('pro_fresh')$$,
  'a Pro user keeps only events younger than 1 day');
select is(
  (select count(*)::int from public.remote_audit_events
    where owner_user_id = '00000000-0000-0000-0000-0000000000c3'),
  0, 'a member of an organization without Team gets the 1-day window');

select function_privs_are('public', 'prune_remote_audit_events', array[]::text[], 'authenticated',
  array[]::text[], 'authenticated cannot prune audit events');

select * from finish();
rollback;
