-- A subscription whose plan does not fit its owner (a user on Team, an
-- organization on Pro) is not written; the webhook alerts instead
-- (044_owner_plan_mismatch.sql).
begin;
select plan(6);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000005a1', 'mismatch-user@example.com');
insert into public.organizations (id, name) values
  ('00000000-0000-0000-0000-0000000005b1', 'Mismatch Org');
insert into public.billing_customers (owner_type, owner_id, stripe_customer_id) values
  ('user', '00000000-0000-0000-0000-0000000005a1', 'cus_mm_user'),
  ('organization', '00000000-0000-0000-0000-0000000005b1', 'cus_mm_org');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(public.billing_record_subscription_event('evt_mm_1', 'customer.subscription.created',
  now() - interval '1 minute', 'sub_mm_user', 'cus_mm_user', 'pro', 'active',
  now() + interval '30 days', false, null), 'applied', 'a user Pro subscription applies');
select is(public.billing_record_subscription_event('evt_mm_2', 'customer.subscription.updated',
  now(), 'sub_mm_user', 'cus_mm_user', 'team', 'active',
  now() + interval '30 days', false, null), 'owner_plan_mismatch',
  'moving a user subscription to Team is refused as a mismatch');
select is((select plan from public.billing_subscriptions where stripe_subscription_id = 'sub_mm_user'),
  'pro', 'the user subscription keeps its Pro plan');
select is(public.has_pro('00000000-0000-0000-0000-0000000005a1'), true,
  'the user keeps Pro while someone looks at the mismatch');
select is(public.billing_record_subscription_event('evt_mm_3', 'customer.subscription.created',
  now(), 'sub_mm_org', 'cus_mm_org', 'pro', 'active',
  now() + interval '30 days', false, null), 'owner_plan_mismatch',
  'an organization on Pro is refused as a mismatch');
select is((select count(*)::int from public.billing_events where stripe_event_id in ('evt_mm_2', 'evt_mm_3')),
  2, 'mismatched events are recorded so a retry does not repeat them');

select * from finish();
rollback;
