-- The stripe-webhook write path from 025_billing_entitlement.sql. Each
-- function records the Stripe event id, so a replay changes nothing. Events
-- may arrive in any order: a late older event never overwrites a newer one,
-- an ended subscription never comes back, and a subscription that arrives
-- before its customer mapping is attached when the mapping lands.
begin;
select plan(39);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000b1', 'webhook-a@example.com'),
  ('00000000-0000-0000-0000-0000000000b2', 'webhook-b@example.com'),
  ('00000000-0000-0000-0000-0000000000b3', 'webhook-c@example.com');

-- ── Grants ────────────────────────────────────────────────────────────────
select function_privs_are('public', 'billing_record_subscription_event',
  array['text', 'text', 'timestamptz', 'text', 'text', 'text', 'text', 'timestamptz', 'boolean', 'timestamptz'],
  'anon', array[]::text[], 'anon cannot execute billing_record_subscription_event');
select function_privs_are('public', 'billing_record_subscription_event',
  array['text', 'text', 'timestamptz', 'text', 'text', 'text', 'text', 'timestamptz', 'boolean', 'timestamptz'],
  'authenticated', array[]::text[], 'authenticated cannot execute billing_record_subscription_event');
select function_privs_are('public', 'billing_record_subscription_event',
  array['text', 'text', 'timestamptz', 'text', 'text', 'text', 'text', 'timestamptz', 'boolean', 'timestamptz'],
  'service_role', array['EXECUTE'], 'service_role can execute billing_record_subscription_event');
select function_privs_are('public', 'billing_record_checkout_completed',
  array['text', 'text', 'text', 'uuid'],
  'anon', array[]::text[], 'anon cannot execute billing_record_checkout_completed');
select function_privs_are('public', 'billing_record_checkout_completed',
  array['text', 'text', 'text', 'uuid'],
  'authenticated', array[]::text[], 'authenticated cannot execute billing_record_checkout_completed');
select function_privs_are('public', 'billing_record_checkout_completed',
  array['text', 'text', 'text', 'uuid'],
  'service_role', array['EXECUTE'], 'service_role can execute billing_record_checkout_completed');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

-- ── A subscription that arrives before its customer mapping ───────────────
select is(
  public.billing_record_subscription_event('evt_sub_created', 'customer.subscription.created',
    '2026-10-01 00:00:00+00', 'sub_a', 'cus_a', 'pro', 'trialing',
    '2026-10-15 00:00:00+00', false, '2026-10-15 00:00:00+00'),
  'applied', 'subscription.created is applied');
select is(
  (select owner_id from public.billing_subscriptions where stripe_subscription_id = 'sub_a'),
  null, 'the subscription has no owner until the customer is mapped');
select is(public.has_pro('00000000-0000-0000-0000-0000000000b1'), false,
  'an unmapped subscription entitles nobody');

select is(
  public.billing_record_checkout_completed('evt_checkout_a', 'cus_a', 'user', '00000000-0000-0000-0000-0000000000b1'),
  'applied', 'checkout.session.completed maps the customer');
select is(
  (select owner_id from public.billing_subscriptions where stripe_subscription_id = 'sub_a'),
  '00000000-0000-0000-0000-0000000000b1'::uuid, 'the earlier subscription is attached to the owner');
select is(public.has_pro('00000000-0000-0000-0000-0000000000b1'), true,
  'the owner is entitled once the mapping lands');
select isnt(
  (select trial_used_at from public.billing_customers where stripe_customer_id = 'cus_a'),
  null, 'checkout records the used trial when the subscription has one');

-- ── Replays change nothing ────────────────────────────────────────────────
select is(
  public.billing_record_checkout_completed('evt_checkout_a', 'cus_a', 'user', '00000000-0000-0000-0000-0000000000b1'),
  'duplicate', 'a replayed checkout event is a duplicate');
select is(
  public.billing_record_subscription_event('evt_sub_created', 'customer.subscription.created',
    '2026-10-01 00:00:00+00', 'sub_a', 'cus_a', 'pro', 'canceled',
    '2026-10-15 00:00:00+00', false, null),
  'duplicate', 'a replayed subscription event is a duplicate even with a changed body');
select is(
  (select status from public.billing_subscriptions where stripe_subscription_id = 'sub_a'),
  'trialing', 'the replay did not change the subscription');
select is((select count(*)::int from public.billing_events), 2,
  'billing_events holds one row per distinct event');

-- ── Ordering ──────────────────────────────────────────────────────────────
select is(
  public.billing_record_subscription_event('evt_sub_active', 'customer.subscription.updated',
    '2026-10-15 00:00:10+00', 'sub_a', 'cus_a', 'pro', 'active',
    '2026-11-15 00:00:00+00', false, '2026-10-15 00:00:00+00'),
  'applied', 'a newer update is applied');
select is(
  public.billing_record_subscription_event('evt_sub_late', 'customer.subscription.updated',
    '2026-10-15 00:00:05+00', 'sub_a', 'cus_a', 'pro', 'past_due',
    '2026-10-15 00:00:00+00', false, '2026-10-15 00:00:00+00'),
  'stale', 'an older update that arrives late is stale');
select is(
  (select status from public.billing_subscriptions where stripe_subscription_id = 'sub_a'),
  'active', 'the stale update did not overwrite the newer status');

select is(
  public.billing_record_subscription_event('evt_sub_canceling', 'customer.subscription.updated',
    '2026-10-20 00:00:00+00', 'sub_a', 'cus_a', 'pro', 'active',
    '2026-11-15 00:00:00+00', true, '2026-10-15 00:00:00+00'),
  'applied', 'cancel at period end is applied');
select is(public.has_pro('00000000-0000-0000-0000-0000000000b1'), true,
  'a subscription set to cancel stays entitled until it ends');

select is(
  public.billing_record_subscription_event('evt_sub_deleted', 'customer.subscription.deleted',
    '2026-11-15 00:00:00+00', 'sub_a', 'cus_a', 'pro', 'canceled',
    '2026-11-15 00:00:00+00', true, '2026-10-15 00:00:00+00'),
  'applied', 'subscription.deleted is applied');
select is(public.has_pro('00000000-0000-0000-0000-0000000000b1'), false,
  'entitlement ends when the subscription is deleted');
select is(
  public.billing_record_subscription_event('evt_sub_resurrect', 'customer.subscription.updated',
    '2026-11-15 00:00:00+00', 'sub_a', 'cus_a', 'pro', 'active',
    '2026-12-15 00:00:00+00', false, null),
  'stale', 'an update in the same second as the deletion cannot reactivate it');
select is(
  (select status from public.billing_subscriptions where stripe_subscription_id = 'sub_a'),
  'canceled', 'the subscription stays canceled');

-- ── Owner spoofing ────────────────────────────────────────────────────────
select is(
  public.billing_record_checkout_completed('evt_checkout_steal', 'cus_a', 'user', '00000000-0000-0000-0000-0000000000b2'),
  'owner_conflict', 'a session cannot move a mapped customer to another user');
select is(
  (select owner_id from public.billing_customers where stripe_customer_id = 'cus_a'),
  '00000000-0000-0000-0000-0000000000b1'::uuid, 'the customer keeps its first owner');
select is(
  public.billing_record_checkout_completed('evt_checkout_second_customer', 'cus_a2', 'user', '00000000-0000-0000-0000-0000000000b1'),
  'owner_conflict', 'an owner cannot gain a second Stripe customer');
select is((select count(*)::int from public.billing_customers where stripe_customer_id = 'cus_a2'), 0,
  'the second customer is not mapped');
select is(
  public.billing_record_checkout_completed('evt_checkout_ghost', 'cus_ghost', 'user', '00000000-0000-0000-0000-00000000dead'),
  'unknown_owner', 'a session for a user that does not exist maps nothing');
select is((select count(*)::int from public.billing_customers where stripe_customer_id = 'cus_ghost'), 0,
  'the ghost customer is not mapped');
select throws_ok(
  $$select public.billing_record_checkout_completed('evt_checkout_org', 'cus_org', 'team', '00000000-0000-0000-0000-0000000000b3')$$,
  '22023', null,
  'an owner type other than user or organization is rejected');

-- ── Mapping first, then the subscription ─────────────────────────────────
select is(
  public.billing_record_checkout_completed('evt_checkout_c', 'cus_c', 'user', '00000000-0000-0000-0000-0000000000b3'),
  'applied', 'checkout maps a customer that has no subscription yet');
select is(
  (select trial_used_at from public.billing_customers where stripe_customer_id = 'cus_c'),
  null, 'no trial is recorded before a trialing subscription exists');
select is(
  public.billing_record_subscription_event('evt_sub_c', 'customer.subscription.created',
    '2026-10-02 00:00:00+00', 'sub_c', 'cus_c', 'pro', 'trialing',
    '2026-10-16 00:00:00+00', false, '2026-10-16 00:00:00+00'),
  'applied', 'the subscription is applied');
select is(
  (select owner_id from public.billing_subscriptions where stripe_subscription_id = 'sub_c'),
  '00000000-0000-0000-0000-0000000000b3'::uuid, 'the subscription takes its owner from the mapping');
select isnt(
  (select trial_used_at from public.billing_customers where stripe_customer_id = 'cus_c'),
  null, 'the trialing subscription records the used trial');
select is(public.has_pro('00000000-0000-0000-0000-0000000000b3'), true,
  'the trialing owner is entitled');

reset role;

select * from finish();
rollback;
