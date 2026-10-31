-- Entitlement ends 30 days after current_period_end even if the
-- customer.subscription.deleted webhook never arrives
-- (039_entitlement_period_grace.sql).
begin;
select plan(8);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000004a1', 'grace-recent@example.com'),
  ('00000000-0000-0000-0000-0000000004a2', 'grace-stale@example.com'),
  ('00000000-0000-0000-0000-0000000004a3', 'grace-pastdue@example.com'),
  ('00000000-0000-0000-0000-0000000004a4', 'grace-noperiod@example.com'),
  ('00000000-0000-0000-0000-0000000004a5', 'grace-member@example.com');
insert into public.organizations (id, name) values
  ('00000000-0000-0000-0000-0000000004b1', 'Stale Team');
insert into public.organization_members (org_id, user_id, role) values
  ('00000000-0000-0000-0000-0000000004b1', '00000000-0000-0000-0000-0000000004a5', 'owner');

insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, current_period_end, stripe_event_created)
values
  ('sub_grace_recent', 'cus_grace_recent', 'user', '00000000-0000-0000-0000-0000000004a1', 'pro', 'active', now() - interval '10 days', now()),
  ('sub_grace_stale', 'cus_grace_stale', 'user', '00000000-0000-0000-0000-0000000004a2', 'pro', 'active', now() - interval '31 days', now()),
  ('sub_grace_pastdue', 'cus_grace_pastdue', 'user', '00000000-0000-0000-0000-0000000004a3', 'pro', 'past_due', now() - interval '45 days', now()),
  ('sub_grace_noperiod', 'cus_grace_noperiod', 'user', '00000000-0000-0000-0000-0000000004a4', 'pro', 'active', null, now()),
  ('sub_grace_team', 'cus_grace_team', 'organization', '00000000-0000-0000-0000-0000000004b1', 'team', 'active', now() - interval '31 days', now());

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(public.has_pro('00000000-0000-0000-0000-0000000004a1'), true,
  'Pro holds 10 days past the period end');
select is(public.has_pro('00000000-0000-0000-0000-0000000004a2'), false,
  'Pro ends 31 days past the period end');
select is(public.has_pro('00000000-0000-0000-0000-0000000004a3'), false,
  'a past_due subscription 45 days past its period end no longer entitles');
select is(public.has_pro('00000000-0000-0000-0000-0000000004a4'), true,
  'a subscription without a period end still entitles');
select is(public.organization_has_team('00000000-0000-0000-0000-0000000004b1'), false,
  'Team ends 31 days past the period end');
select is(public.has_pro('00000000-0000-0000-0000-0000000004a5'), false,
  'a member of a stale Team is not Pro');

reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"role":"authenticated","sub":"00000000-0000-0000-0000-0000000004a2"}', true);
select is((select plan from public.get_current_user_subscription()), 'free',
  'the desktop view shows free for a stale subscription');
select is((select status from public.get_current_user_subscription()), 'inactive',
  'the desktop view shows inactive for a stale subscription');

select * from finish();
rollback;
