-- A second live subscription of the same plan for one owner is still
-- recorded (the owner is billed for it) but reported as
-- 'duplicate_subscription' so the webhook alerts (045_duplicate_subscription.sql).
begin;
select plan(4);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000006a1', 'dup-sub@example.com');
insert into public.billing_customers (owner_type, owner_id, stripe_customer_id) values
  ('user', '00000000-0000-0000-0000-0000000006a1', 'cus_dup');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(public.billing_record_subscription_event('evt_dup_1', 'customer.subscription.created',
  now(), 'sub_dup_1', 'cus_dup', 'pro', 'active', now() + interval '30 days', false, null),
  'applied', 'the first Pro subscription applies');
select is(public.billing_record_subscription_event('evt_dup_2', 'customer.subscription.created',
  now(), 'sub_dup_2', 'cus_dup', 'pro', 'active', now() + interval '30 days', false, null),
  'duplicate_subscription', 'a second live Pro subscription is reported');
select is((select count(*)::int from public.billing_subscriptions where stripe_customer_id = 'cus_dup'),
  2, 'both subscriptions are recorded');
select is(public.billing_record_subscription_event('evt_dup_3', 'customer.subscription.updated',
  now() + interval '1 second', 'sub_dup_1', 'cus_dup', 'pro', 'active', now() + interval '30 days', false, null),
  'duplicate_subscription', 'updates keep reporting while two are live');

select * from finish();
rollback;
