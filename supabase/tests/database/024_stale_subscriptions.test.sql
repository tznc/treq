-- billing_stale_subscriptions lists subscriptions that still entitle but whose
-- period ended more than a day ago, for the reconcile job
-- (043_billing_reconcile.sql).
begin;
select plan(3);

insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, plan, status, current_period_end, stripe_event_created)
values
  ('sub_rc_stale', 'cus_rc_1', 'pro', 'active', now() - interval '3 days', now()),
  ('sub_rc_fresh', 'cus_rc_2', 'pro', 'active', now() + interval '10 days', now()),
  ('sub_rc_ended', 'cus_rc_3', 'pro', 'canceled', now() - interval '3 days', now()),
  ('sub_rc_pastdue', 'cus_rc_4', 'team', 'past_due', now() - interval '40 days', now());

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select set_eq(
  $$select id from public.billing_stale_subscriptions(100) as id where id like 'sub_rc_%'$$,
  array['sub_rc_stale', 'sub_rc_pastdue'],
  'lists live subscriptions whose period ended over a day ago');
select is((select count(*)::int from public.billing_stale_subscriptions(1)), 1,
  'respects the limit');

reset role;
select function_privs_are('public', 'billing_stale_subscriptions', array['integer'],
  'authenticated', array[]::text[], 'authenticated cannot list stale subscriptions');

select * from finish();
rollback;
