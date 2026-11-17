-- Entitlement and read access for 025_billing_entitlement.sql: has_pro for
-- every Stripe subscription status, row-level security on the billing
-- tables, and the public.subscriptions contract the desktop app reads.
begin;
select plan(48);

-- Fixtures, created as postgres (bypasses RLS). One user per status.
insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000a1', 'billing-trialing@example.com'),
  ('00000000-0000-0000-0000-0000000000a2', 'billing-active@example.com'),
  ('00000000-0000-0000-0000-0000000000a3', 'billing-past-due@example.com'),
  ('00000000-0000-0000-0000-0000000000a4', 'billing-canceled@example.com'),
  ('00000000-0000-0000-0000-0000000000a5', 'billing-unpaid@example.com'),
  ('00000000-0000-0000-0000-0000000000a6', 'billing-incomplete@example.com'),
  ('00000000-0000-0000-0000-0000000000a7', 'billing-incomplete-expired@example.com'),
  ('00000000-0000-0000-0000-0000000000a8', 'billing-paused@example.com'),
  ('00000000-0000-0000-0000-0000000000a9', 'billing-team-plan@example.com'),
  ('00000000-0000-0000-0000-0000000000aa', 'billing-free@example.com'),
  ('00000000-0000-0000-0000-0000000000ab', 'billing-canceling@example.com');

insert into public.billing_customers (owner_type, owner_id, stripe_customer_id, trial_used_at) values
  ('user', '00000000-0000-0000-0000-0000000000a1', 'cus_trialing', now()),
  ('user', '00000000-0000-0000-0000-0000000000a2', 'cus_active', null);

insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status,
   current_period_end, cancel_at_period_end, trial_end, stripe_event_created)
values
  ('sub_trialing', 'cus_trialing', 'user', '00000000-0000-0000-0000-0000000000a1', 'pro', 'trialing',
   '2026-11-01 00:00:00+00', false, '2026-11-01 00:00:00+00', now()),
  ('sub_active', 'cus_active', 'user', '00000000-0000-0000-0000-0000000000a2', 'pro', 'active',
   '2026-11-02 00:00:00+00', false, null, now()),
  ('sub_past_due', 'cus_past_due', 'user', '00000000-0000-0000-0000-0000000000a3', 'pro', 'past_due',
   '2026-11-03 00:00:00+00', false, null, now()),
  ('sub_canceled', 'cus_canceled', 'user', '00000000-0000-0000-0000-0000000000a4', 'pro', 'canceled',
   '2026-10-01 00:00:00+00', false, null, now()),
  ('sub_unpaid', 'cus_unpaid', 'user', '00000000-0000-0000-0000-0000000000a5', 'pro', 'unpaid',
   '2026-11-05 00:00:00+00', false, null, now()),
  ('sub_incomplete', 'cus_incomplete', 'user', '00000000-0000-0000-0000-0000000000a6', 'pro', 'incomplete',
   '2026-11-06 00:00:00+00', false, null, now()),
  ('sub_incomplete_expired', 'cus_incomplete_expired', 'user', '00000000-0000-0000-0000-0000000000a7', 'pro', 'incomplete_expired',
   '2026-11-07 00:00:00+00', false, null, now()),
  ('sub_paused', 'cus_paused', 'user', '00000000-0000-0000-0000-0000000000a8', 'pro', 'paused',
   '2026-11-08 00:00:00+00', false, null, now()),
  -- Team belongs to organizations. A user-owned Team row grants nothing.
  ('sub_team', 'cus_team', 'user', '00000000-0000-0000-0000-0000000000a9', 'team', 'active',
   '2026-11-09 00:00:00+00', false, null, now()),
  -- An organization that happens to share the free user's id owns nothing for them.
  ('sub_org', 'cus_org', 'organization', '00000000-0000-0000-0000-0000000000aa', 'pro', 'active',
   '2026-11-10 00:00:00+00', false, null, now()),
  ('sub_canceling', 'cus_canceling', 'user', '00000000-0000-0000-0000-0000000000ab', 'pro', 'active',
   '2026-11-11 00:00:00+00', true, null, now()),
  -- Not yet mapped to an owner.
  ('sub_orphan', 'cus_orphan', null, null, 'pro', 'active',
   '2026-11-12 00:00:00+00', false, null, now());

-- ── has_pro shape and grants ──────────────────────────────────────────────
select is_definer('public', 'has_pro', array['uuid'], 'has_pro is security definer');
select ok(
  (select proconfig @> array['search_path=""'] from pg_proc where oid = 'public.has_pro(uuid)'::regprocedure),
  'has_pro pins an empty search_path');
select function_privs_are('public', 'has_pro', array['uuid'], 'anon', array[]::text[],
  'anon cannot execute has_pro');
select function_privs_are('public', 'has_pro', array['uuid'], 'authenticated', array['EXECUTE'],
  'authenticated can execute has_pro');
select function_privs_are('public', 'has_pro', array['uuid'], 'service_role', array['EXECUTE'],
  'service_role can execute has_pro');

-- ── has_pro for every status, as the service role ─────────────────────────
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(public.has_pro('00000000-0000-0000-0000-0000000000a1'), true, 'trialing Pro is entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a2'), true, 'active Pro is entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a3'), true, 'past_due Pro stays entitled while Stripe retries');
select is(public.has_pro('00000000-0000-0000-0000-0000000000ab'), true, 'Pro set to cancel at period end stays entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a4'), false, 'canceled Pro is not entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a5'), false, 'unpaid Pro is not entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a6'), false, 'incomplete Pro is not entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a7'), false, 'incomplete_expired Pro is not entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a8'), false, 'paused Pro is not entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a9'), false, 'a user-owned Team subscription grants nothing');
select is(public.has_pro('00000000-0000-0000-0000-0000000000aa'), false, 'a user with no subscription is not entitled');
select is(public.has_pro(null), false, 'a null user id is not entitled');

-- ── has_pro as a signed-in user ───────────────────────────────────────────
reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a1","role":"authenticated"}', true);

select is(public.has_pro('00000000-0000-0000-0000-0000000000a1'), true,
  'a trialing user sees their own entitlement');
select is(public.has_pro('00000000-0000-0000-0000-0000000000a2'), false,
  'a user cannot learn whether another user is entitled');

-- ── RLS: own rows only, read only ─────────────────────────────────────────
select results_eq(
  $$select stripe_subscription_id from public.billing_subscriptions$$,
  $$values ('sub_trialing'::text)$$,
  'a user sees only their own subscription rows');
select results_eq(
  $$select stripe_customer_id from public.billing_customers$$,
  $$values ('cus_trialing'::text)$$,
  'a user sees only their own customer row');
select throws_ok(
  $$select * from public.billing_events$$,
  '42501', null,
  'a user cannot read billing_events');
select throws_ok(
  $$insert into public.billing_subscriptions
      (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
    values ('sub_forged', 'cus_forged', 'user', '00000000-0000-0000-0000-0000000000a1', 'pro', 'active', now())$$,
  '42501', null,
  'a user cannot insert a subscription');
select throws_ok(
  $$update public.billing_subscriptions set status = 'active' where stripe_subscription_id = 'sub_trialing'$$,
  '42501', null,
  'a user cannot update their subscription');
select throws_ok(
  $$update public.billing_customers set trial_used_at = null$$,
  '42501', null,
  'a user cannot clear trial_used_at');
select throws_ok(
  $$insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
    values ('user', '00000000-0000-0000-0000-0000000000a1', 'cus_someone_elses')$$,
  '42501', null,
  'a user cannot map a Stripe customer to themselves');
select throws_ok(
  $$delete from public.billing_subscriptions$$,
  '42501', null,
  'a user cannot delete subscription rows');

reset role;
set local role anon;
select set_config('request.jwt.claims', '{"role":"anon"}', true);

select throws_ok(
  $$select * from public.billing_subscriptions$$,
  '42501', null,
  'anon cannot read billing_subscriptions');
select throws_ok(
  $$select * from public.billing_customers$$,
  '42501', null,
  'anon cannot read billing_customers');
select throws_ok(
  $$select public.has_pro('00000000-0000-0000-0000-0000000000a1')$$,
  '42501', null,
  'anon cannot call has_pro');

reset role;

select ok((select relrowsecurity from pg_class where oid = 'public.billing_customers'::regclass),
  'RLS enabled on public.billing_customers');
select ok((select relrowsecurity from pg_class where oid = 'public.billing_subscriptions'::regclass),
  'RLS enabled on public.billing_subscriptions');
select ok((select relrowsecurity from pg_class where oid = 'public.billing_events'::regclass),
  'RLS enabled on public.billing_events');
select table_privs_are('public', 'billing_subscriptions', 'authenticated', array['SELECT'],
  'authenticated can only select billing_subscriptions');
select table_privs_are('public', 'billing_customers', 'authenticated', array['SELECT'],
  'authenticated can only select billing_customers');
select table_privs_are('public', 'billing_events', 'authenticated', array[]::text[],
  'authenticated has no privileges on billing_events');
select table_privs_are('public', 'billing_subscriptions', 'anon', array[]::text[],
  'anon has no privileges on billing_subscriptions');

-- ── public.subscriptions contract ─────────────────────────────────────────
select columns_are('public', 'subscriptions', array['plan', 'status', 'current_period_end'],
  'public.subscriptions keeps its three columns');
select col_type_is('public', 'subscriptions', 'current_period_end', 'timestamp without time zone',
  'current_period_end keeps its type');
select ok(
  not exists (
    select 1 from pg_depend d
    join pg_class c on c.oid = d.refobjid
    join pg_namespace n on n.oid = c.relnamespace
    where d.objid = 'public.get_current_user_subscription()'::regprocedure
      and n.nspname = 'treq_internal'
  )
  and position('treq_internal.stripe_' in pg_get_functiondef('public.get_current_user_subscription()'::regprocedure)) = 0,
  'get_current_user_subscription no longer reads the Stripe foreign tables');

set local role authenticated;

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a1","role":"authenticated"}', true);
select results_eq(
  $$select plan, status, current_period_end from public.subscriptions$$,
  $$values ('pro'::text, 'trialing'::text, '2026-11-01 00:00:00'::timestamp)$$,
  'a trialing user reads pro / trialing with the trial end as the period end');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a2","role":"authenticated"}', true);
select results_eq(
  $$select plan, status, current_period_end from public.subscriptions$$,
  $$values ('pro'::text, 'active'::text, '2026-11-02 00:00:00'::timestamp)$$,
  'an active user reads pro / active with the renewal date');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a3","role":"authenticated"}', true);
select results_eq(
  $$select plan, status from public.subscriptions$$,
  $$values ('pro'::text, 'past_due'::text)$$,
  'a past_due user reads pro / past_due');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000ab","role":"authenticated"}', true);
select results_eq(
  $$select plan, status, current_period_end from public.subscriptions$$,
  $$values ('pro'::text, 'canceled'::text, '2026-11-11 00:00:00'::timestamp)$$,
  'cancel at period end reads pro / canceled with the end date');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a4","role":"authenticated"}', true);
select results_eq(
  $$select plan, status, current_period_end from public.subscriptions$$,
  $$values ('free'::text, 'inactive'::text, null::timestamp)$$,
  'an ended subscription reads free / inactive');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a5","role":"authenticated"}', true);
select results_eq(
  $$select plan, status from public.subscriptions$$,
  $$values ('free'::text, 'inactive'::text)$$,
  'an unpaid subscription reads free / inactive');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a9","role":"authenticated"}', true);
select results_eq(
  $$select plan, status from public.subscriptions$$,
  $$values ('free'::text, 'inactive'::text)$$,
  'a user-owned Team row reads free / inactive');

select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000aa","role":"authenticated"}', true);
select results_eq(
  $$select plan, status, current_period_end from public.subscriptions$$,
  $$values ('free'::text, 'inactive'::text, null::timestamp)$$,
  'a user with no subscription reads exactly one free / inactive row');

reset role;

select * from finish();
rollback;
