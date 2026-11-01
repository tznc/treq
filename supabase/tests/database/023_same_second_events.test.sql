-- Two subscription events created in the same second that disagree cannot
-- be ordered, so the second is answered 'tie' without being recorded, and
-- the webhook writes a fresh Stripe read through
-- billing_record_subscription_snapshot (042_same_second_events.sql).
begin;
select plan(10);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000007a1', 'tie@example.com');
insert into public.billing_customers (owner_type, owner_id, stripe_customer_id) values
  ('user', '00000000-0000-0000-0000-0000000007a1', 'cus_tie');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select set_config('test.t', now()::text, true);

select is(public.billing_record_subscription_event('evt_tie_1', 'customer.subscription.updated',
  current_setting('test.t')::timestamptz, 'sub_tie', 'cus_tie', 'pro', 'active',
  now() + interval '30 days', false, null), 'applied', 'the first event applies');
select is(public.billing_record_subscription_event('evt_tie_2', 'customer.subscription.updated',
  current_setting('test.t')::timestamptz, 'sub_tie', 'cus_tie', 'pro', 'active',
  now() + interval '30 days', true, null), 'tie', 'a disagreeing event from the same second is a tie');
select is((select cancel_at_period_end from public.billing_subscriptions where stripe_subscription_id = 'sub_tie'),
  false, 'a tie does not write');
select is((select count(*)::int from public.billing_events where stripe_event_id = 'evt_tie_2'),
  0, 'a tie is not recorded, so a retry asks again');
select is(public.billing_record_subscription_event('evt_tie_3', 'customer.subscription.updated',
  current_setting('test.t')::timestamptz, 'sub_tie', 'cus_tie', 'pro', 'active',
  (select current_period_end from public.billing_subscriptions where stripe_subscription_id = 'sub_tie'), false, null),
  'applied', 'an agreeing event from the same second applies');

select is(public.billing_record_subscription_snapshot('sub_tie', 'cus_tie', 'pro', 'active',
  now() + interval '30 days', true, null, current_setting('test.t')::timestamptz + interval '1 second'),
  'applied', 'a fresh Stripe read applies');
select is((select cancel_at_period_end from public.billing_subscriptions where stripe_subscription_id = 'sub_tie'),
  true, 'the snapshot wins');
select is(public.billing_record_subscription_snapshot('sub_tie', 'cus_tie', 'pro', 'active',
  now() + interval '30 days', false, null, current_setting('test.t')::timestamptz - interval '1 minute'),
  'stale', 'an older read does not overwrite a newer write');

select is(public.billing_record_subscription_event('evt_tie_del', 'customer.subscription.deleted',
  (select stripe_event_created from public.billing_subscriptions where stripe_subscription_id = 'sub_tie'),
  'sub_tie', 'cus_tie', 'pro', 'canceled', now(), false, null),
  'applied', 'a deletion from the same second is never a tie: an ended subscription stays ended');

reset role;
select function_privs_are('public', 'billing_record_subscription_snapshot',
  array['text','text','text','text','timestamptz','boolean','timestamptz','timestamptz','timestamptz'],
  'authenticated', array[]::text[], 'authenticated cannot write snapshots');

select * from finish();
rollback;
