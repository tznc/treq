-- Ledger billing: provider-time transitions, price splits, report grace,
-- terminal report states and the Stripe-period spend window (038, 040).
begin;
select plan(26);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000f1', 'bf-1@example.com'),
  ('00000000-0000-0000-0000-0000000000f2', 'bf-2@example.com'),
  ('00000000-0000-0000-0000-0000000000f3', 'bf-3@example.com'),
  ('00000000-0000-0000-0000-0000000000f4', 'bf-4@example.com');

-- The seeded prices start when the migration ran; move them back a day so
-- the tests can backdate transitions and price changes.
update public.machine_type_prices set effective_from = now() - interval '1 day'
 where machine_type_id in ('boat:small', 'boat:default') and effective_to is null;

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status,
   tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000f001', '00000000-0000-0000-0000-0000000000f1',
   'boat', 'boat-f1', 'us_east', 'small', 'provisioning', 's', 'boat:small'),
  ('00000000-0000-0000-0000-00000000f002', '00000000-0000-0000-0000-0000000000f2',
   'boat', 'boat-f2', 'us_east', 'small', 'provisioning', 'm', 'boat:default'),
  ('00000000-0000-0000-0000-00000000f003', '00000000-0000-0000-0000-0000000000f3',
   'boat', 'boat-f3', 'us_east', 'small', 'provisioning', 's', 'boat:small');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region,
   provider_resource_id, state, role)
values
  ('00000000-0000-0000-0000-00000000f101', '00000000-0000-0000-0000-00000000f001',
   '00000000-0000-0000-0000-0000000000f1', 'boat', 'boat:small', 'us_east', 'boat-f1', 'provisioning', 'active'),
  ('00000000-0000-0000-0000-00000000f102', '00000000-0000-0000-0000-00000000f002',
   '00000000-0000-0000-0000-0000000000f2', 'boat', 'boat:default', 'us_east', 'boat-f2', 'provisioning', 'active'),
  ('00000000-0000-0000-0000-00000000f103', '00000000-0000-0000-0000-00000000f003',
   '00000000-0000-0000-0000-0000000000f3', 'boat', 'boat:small', 'us_east', 'boat-f3', 'provisioning', 'active');

-- ── Disk quota comes from the machine type ───────────────────────────
select is((select disk_quota_gb from public.remote_instances where id = '00000000-0000-0000-0000-00000000f002'),
  50, 'disk_quota_gb is the machine type''s disk_gb, not the old 5 GB base allocation');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

-- boat:small gets a stopped-storage price from 20 hours ago.
select public.set_machine_type_price('boat:small', 5000, 7, now() - interval '20 hours');

-- ── Transitions at the provider's time ───────────────────────────────
select public.set_placement_state('00000000-0000-0000-0000-00000000f101', 'ready', null, now() - interval '10 hours');
select is((select started_at from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and kind = 'running'),
  now() - interval '10 hours', 'an out-of-band start opens the interval at the provider''s time');

select public.set_placement_state('00000000-0000-0000-0000-00000000f101', 'suspended', null, now() - interval '9 hours');
select is((select ended_at from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and kind = 'running'),
  now() - interval '9 hours', 'a provider stop closes the interval at the provider''s time, not now()');

-- ── Stopped storage bills the machine type's disk ────────────────────
select results_eq(
  $$ select disk_gb, started_at from public.usage_intervals
      where placement_id = '00000000-0000-0000-0000-00000000f101' and kind = 'stopped_storage' $$,
  $$ values (12, now() - interval '9 hours') $$,
  'a stop with no disk size bills the machine type''s disk_gb');
select is(
  (select round(extract(epoch from now() - u.started_at) * p.stopped_nanousd_per_gb_sec * u.disk_gb)::bigint
     from public.usage_intervals u join public.machine_type_prices p on p.id = u.price_id
    where u.placement_id = '00000000-0000-0000-0000-00000000f101' and u.kind = 'stopped_storage'),
  (9 * 3600 * 7 * 12)::bigint, 'nine stopped hours bill 12 GB at the stopped price');

-- p_at is clamped to now() and to the interval start.
select public.set_placement_state('00000000-0000-0000-0000-00000000f101', 'ready', null, now() + interval '1 hour');
select is((select ended_at from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and kind = 'stopped_storage'),
  now(), 'a future p_at is clamped to now()');
select is((select started_at from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and kind = 'running' and ended_at is null),
  now(), 'a start at a future p_at opens at now()');
select lives_ok(
  $$ select public.set_placement_state('00000000-0000-0000-0000-00000000f101', 'suspended', null, now() - interval '30 hours') $$,
  'a p_at before the interval start does not fail');
select is((select count(*)::int from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and ended_at < started_at),
  0, 'no interval ends before it starts');
select is((select count(*)::int from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and ended_at is null and kind = 'stopped_storage'),
  1, 'the stop still opens stopped storage');

-- An explicit disk size still wins.
select public.set_placement_state('00000000-0000-0000-0000-00000000f101', 'ready');
select public.set_placement_state('00000000-0000-0000-0000-00000000f101', 'suspended', 30);
select is((select disk_gb from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f101' and kind = 'stopped_storage' and ended_at is null),
  30, 'an explicit p_disk_gb is kept');

-- ── A price change splits running intervals ──────────────────────────
select public.set_placement_state('00000000-0000-0000-0000-00000000f102', 'ready', null, now() - interval '2 hours');
select public.set_machine_type_price('boat:default', 20000, 0, now() - interval '1 hour');
select results_eq(
  $$ select p.running_nanousd_per_sec, u.started_at, u.ended_at
       from public.usage_intervals u join public.machine_type_prices p on p.id = u.price_id
      where u.placement_id = '00000000-0000-0000-0000-00000000f102' order by u.started_at $$,
  $$ values (10000::bigint, now() - interval '2 hours', now() - interval '1 hour'),
            (20000::bigint, now() - interval '1 hour', null::timestamptz) $$,
  'the open interval closes at effective_from and continues on the new price');
select public.set_placement_state('00000000-0000-0000-0000-00000000f102', 'suspended', null, now() - interval '40 minutes');
select is(
  (select round(sum(extract(epoch from u.ended_at - u.started_at) * p.running_nanousd_per_sec))::bigint
     from public.usage_intervals u join public.machine_type_prices p on p.id = u.price_id
    where u.placement_id = '00000000-0000-0000-0000-00000000f102'),
  (3600 * 10000 + 1200 * 20000)::bigint, 'a mid-interval price change bills both rates');

-- A price that ended while nothing changed state is split when reports build.
select public.set_placement_state('00000000-0000-0000-0000-00000000f102', 'ready', null, now() - interval '30 minutes');
reset role;
-- Simulate a future-dated price that has now taken effect.
update public.machine_type_prices set effective_to = now() - interval '10 minutes'
 where machine_type_id = 'boat:default' and effective_to is null;
insert into public.machine_type_prices (machine_type_id, running_nanousd_per_sec, effective_from)
values ('boat:default', 30000, now() - interval '10 minutes');
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select public.build_usage_reports(date_trunc('hour', now()) - interval '1 hour');
select results_eq(
  $$ select p.running_nanousd_per_sec, u.ended_at
       from public.usage_intervals u join public.machine_type_prices p on p.id = u.price_id
      where u.placement_id = '00000000-0000-0000-0000-00000000f102'
        and u.started_at >= now() - interval '30 minutes' order by u.started_at $$,
  $$ values (20000::bigint, now() - interval '10 minutes'), (30000::bigint, null::timestamptz) $$,
  'build_usage_reports splits an interval whose price has ended');

-- ── Hours are built only after a grace period ────────────────────────
select public.build_usage_reports(date_trunc('hour', now()) + interval '3 minutes');
select is((select count(*)::int from public.usage_reports
            where owner_user_id = '00000000-0000-0000-0000-0000000000f2'
              and hour = date_trunc('hour', now()) - interval '1 hour'),
  0, 'the hour that ended 3 minutes ago is not built yet');
select public.build_usage_reports(date_trunc('hour', now()) + interval '6 minutes');
select is((select count(*)::int from public.usage_reports
            where owner_user_id = '00000000-0000-0000-0000-0000000000f2'
              and hour = date_trunc('hour', now()) - interval '1 hour'),
  1, 'it is built once 5 minutes have passed');

-- ── Deleting keeps billing until the delete succeeds ─────────────────
select public.set_placement_state('00000000-0000-0000-0000-00000000f103', 'ready', null, now() - interval '1 hour');
select public.set_placement_state('00000000-0000-0000-0000-00000000f103', 'deleting');
select is((select count(*)::int from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f103' and kind = 'running' and ended_at is null),
  1, 'deleting keeps the running interval open');
select public.set_placement_state('00000000-0000-0000-0000-00000000f103', 'ready');
select is((select count(*)::int from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f103'),
  1, 'a failed delete restored to ready continues the same interval');
select public.set_placement_state('00000000-0000-0000-0000-00000000f103', 'deleted');
select is((select count(*)::int from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000f103' and ended_at is null),
  0, 'deleted closes it');

-- ── Terminal report states ───────────────────────────────────────────
select has_column('public', 'usage_reports', 'attempts', 'usage_reports counts send attempts');
select lives_ok(
  $$ update public.usage_reports set stripe_status = 'no_customer'
      where owner_user_id = '00000000-0000-0000-0000-0000000000f2' $$,
  'no_customer is a stripe_status');
select throws_ok(
  $$ update public.usage_reports set stripe_status = 'bogus'
      where owner_user_id = '00000000-0000-0000-0000-0000000000f2' $$,
  '23514', null, 'unknown stripe_status values are refused');

-- ── The spend-cap window follows the Stripe period ───────────────────
select public.billing_record_subscription_event(
  p_event_id => 'evt_bf_period', p_event_type => 'customer.subscription.created',
  p_event_created => now(), p_subscription_id => 'sub_bf', p_customer_id => 'cus_bf',
  p_plan => 'pro', p_status => 'active',
  p_current_period_end => date_trunc('hour', now()) + interval '27 days',
  p_cancel_at_period_end => false, p_trial_end => null,
  p_current_period_start => date_trunc('hour', now()) - interval '3 days' + interval '30 minutes');
select is((select current_period_start from public.billing_subscriptions where stripe_subscription_id = 'sub_bf'),
  date_trunc('hour', now()) - interval '3 days' + interval '30 minutes',
  'the webhook stores current_period_start');
reset role;
update public.billing_subscriptions set owner_type = 'user', owner_id = '00000000-0000-0000-0000-0000000000f4'
 where stripe_subscription_id = 'sub_bf';
insert into public.usage_reports (identifier, owner_user_id, hour, seconds, provider_cost_nanousd, billed_nanousd)
values
  ('f4:before', '00000000-0000-0000-0000-0000000000f4',
   date_trunc('hour', now()) - interval '3 days', 3600, 1000, 7000000000),
  ('f4:in', '00000000-0000-0000-0000-0000000000f4',
   date_trunc('hour', now()) - interval '3 days' + interval '1 hour', 3600, 1000, 5000000000);
select is((treq_internal.spend_status('00000000-0000-0000-0000-0000000000f4', now())->>'month_start')::timestamptz,
  date_trunc('hour', now()) - interval '3 days' + interval '1 hour',
  'the cap window starts at the first full hour of the Stripe period');
select is(treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000000f4', now()),
  5000000000::bigint, 'spend counts only the hours in the Stripe period');
select is((treq_internal.spend_status('00000000-0000-0000-0000-0000000000f1', now())->>'month_start')::timestamptz,
  date_trunc('month', now(), 'UTC'), 'without a Pro period the window is the calendar month');

select * from finish();
rollback;
