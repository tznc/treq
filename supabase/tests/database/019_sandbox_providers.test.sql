-- Catalog, placements, the usage ledger and config sync
-- (036, 038, 042).
begin;
select plan(30);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000d1', 'sandbox-pro@example.com'),
  ('00000000-0000-0000-0000-0000000000d2', 'sandbox-other@example.com');

-- ── Catalog and the cheapest-provider default ───────────────────────────

select ok(
  (select count(*) from public.size_tiers) = 4,
  'four size tiers are seeded');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select ok(public.recompute_pro_defaults() > 0, 'recompute_pro_defaults writes defaults');
reset role;

select is(
  (select machine_type_id from public.pro_provider_defaults where tier_id = 's' and region = 'us_east'),
  'boat:small',
  'Boat is the cheapest admitted provider for tier s');

update public.sandbox_providers set status = 'draining' where kind = 'boat';
set local role service_role;
select public.recompute_pro_defaults();
reset role;
select is(
  (select machine_type_id from public.pro_provider_defaults where tier_id = 's' and region = 'us_east'),
  'daytona:s',
  'a draining provider is skipped: tier s falls back to Daytona (Fly is hidden)');
update public.sandbox_providers set status = 'active' where kind = 'boat';
set local role service_role;
select public.recompute_pro_defaults();
reset role;

-- The catalog is readable by a signed-in user, and only shows marked-up prices.
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d1","role":"authenticated"}', true);
select is(
  (select (t->'regions'->0->>'machine_type_id')
     from jsonb_array_elements(public.get_machine_catalog()) t
    where t->>'id' = 's'),
  'boat:small',
  'the catalog offers the default machine type per tier');
select is(
  (select (t->'regions'->0->>'hourly_usd')::numeric
     from jsonb_array_elements(public.get_machine_catalog()) t
    where t->>'id' = 's'),
  0.0216,
  'the catalog price is provider cost x 1.2 ($0.018/h -> $0.0216/h)');
select ok(
  not exists (
    select 1 from jsonb_array_elements(public.get_machine_catalog()) t
     where t::text like '%nanousd%' or t::text like '%markup%'),
  'the catalog hides provider cost and markup');
select throws_ok(
  $$ select * from public.machine_type_prices $$,
  '42501', null,
  'customers cannot read raw prices');
reset role;

-- ── Placements, usage intervals and cutover ─────────────────────────────

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status,
   tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000a001', '00000000-0000-0000-0000-0000000000d1',
   'local', 'dev-treq-d1', 'us_east', 'small', 'ready', 's', 'local:s');

insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region,
   provider_resource_id, state, role)
values
  ('00000000-0000-0000-0000-00000000b001', '00000000-0000-0000-0000-00000000a001',
   '00000000-0000-0000-0000-0000000000d1', 'local', 'local:s', 'us_east',
   'dev-treq-d1', 'provisioning', 'active'),
  ('00000000-0000-0000-0000-00000000b002', '00000000-0000-0000-0000-00000000a001',
   '00000000-0000-0000-0000-0000000000d1', 'boat', 'boat:small', 'us_east',
   null, 'provisioning', 'candidate');
update public.remote_instances
   set active_placement_id = '00000000-0000-0000-0000-00000000b001'
 where id = '00000000-0000-0000-0000-00000000a001';

select throws_ok(
  $$ insert into public.remote_instance_placements
       (instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
     values ('00000000-0000-0000-0000-00000000a001', '00000000-0000-0000-0000-0000000000d1',
             'boat', 'boat:small', 'us_east', 'ready', 'active') $$,
  '23505', null,
  'an instance has at most one active placement');

set local role service_role;
select public.set_placement_state('00000000-0000-0000-0000-00000000b001', 'ready');
reset role;
select is(
  (select count(*)::int from public.usage_intervals
    where placement_id = '00000000-0000-0000-0000-00000000b001' and ended_at is null and kind = 'running'),
  1,
  'an active placement that is ready has one open running interval');

set local role service_role;
select public.set_placement_state('00000000-0000-0000-0000-00000000b001', 'ready');
reset role;
select is(
  (select count(*)::int from public.usage_intervals
    where placement_id = '00000000-0000-0000-0000-00000000b001'),
  1,
  'setting the same state again does not open a second interval');

set local role service_role;
select public.set_placement_state('00000000-0000-0000-0000-00000000b002', 'ready');
reset role;
select is(
  (select count(*)::int from public.usage_intervals
    where placement_id = '00000000-0000-0000-0000-00000000b002'),
  0,
  'a candidate placement is not billed before cutover');

set local role service_role;
select public.set_placement_state('00000000-0000-0000-0000-00000000b001', 'suspended');
reset role;
select is(
  (select count(*)::int from public.usage_intervals
    where placement_id = '00000000-0000-0000-0000-00000000b001' and ended_at is null),
  0,
  'suspending closes the running interval (local bills no stopped storage)');
set local role service_role;
select public.set_placement_state('00000000-0000-0000-0000-00000000b001', 'ready');
reset role;

select throws_ok(
  $$ update public.usage_intervals set started_at = started_at - interval '1 hour'
      where placement_id = '00000000-0000-0000-0000-00000000b001' $$,
  'P0001', null,
  'the ledger rejects edits to started_at');

set local role service_role;
select throws_ok(
  $$ select public.cutover_placement('00000000-0000-0000-0000-00000000a001',
       '00000000-0000-0000-0000-00000000b002', 7) $$,
  'P0001', 'fence_mismatch',
  'cutover with a stale fence is refused');
select is(
  public.cutover_placement('00000000-0000-0000-0000-00000000a001',
    '00000000-0000-0000-0000-00000000b002', 0),
  1::bigint,
  'cutover bumps the fence');
reset role;

select results_eq(
  $$ select provider_kind, machine_type_id, active_placement_id::text
       from public.remote_instances where id = '00000000-0000-0000-0000-00000000a001' $$,
  $$ values ('boat'::text, 'boat:small'::text, '00000000-0000-0000-0000-00000000b002'::text) $$,
  'the instance now points at the Boat placement');
select is(
  (select role from public.remote_instance_placements where id = '00000000-0000-0000-0000-00000000b001'),
  'retiring', 'the source becomes retiring');
select is(
  (select count(*)::int from public.usage_intervals
    where placement_id = '00000000-0000-0000-0000-00000000b001' and ended_at is null),
  0, 'the source stops billing at cutover');
select is(
  (select count(*)::int from public.usage_intervals
    where placement_id = '00000000-0000-0000-0000-00000000b002' and ended_at is null and kind = 'running'),
  1, 'the target starts billing at cutover');

-- ── Usage reports ──────────────────────────────────────────────────────

-- A closed one-hour interval on boat:small, two hours ago.
insert into public.usage_intervals
  (placement_id, owner_user_id, kind, price_id, started_at, ended_at)
select '00000000-0000-0000-0000-00000000b002', '00000000-0000-0000-0000-0000000000d1', 'running', p.id,
       date_trunc('hour', now()) - interval '3 hours',
       date_trunc('hour', now()) - interval '2 hours'
  from public.machine_type_prices p
 where p.machine_type_id = 'boat:small' and p.effective_to is null;

set local role service_role;
select ok(public.build_usage_reports(now()) >= 1, 'reports are built for finished hours');
select is(public.build_usage_reports(now()), 0, 'building again adds nothing (idempotent)');
reset role;

select results_eq(
  $$ select seconds, provider_cost_nanousd, billed_nanousd
       from public.usage_reports
      where owner_user_id = '00000000-0000-0000-0000-0000000000d1'
        and hour = date_trunc('hour', now()) - interval '3 hours' $$,
  $$ values (3600, 18000000::bigint, 21600000::bigint) $$,
  'one hour on boat:small costs $0.018 and bills $0.0216');
select ok(
  not exists (select 1 from public.usage_reports where hour >= date_trunc('hour', now())),
  'the current, unfinished hour is not reported');

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d2","role":"authenticated"}', true);
select is(
  (select count(*)::int from public.get_usage_summary(now() - interval '1 day', now())),
  0, 'another user sees none of this usage');
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d1","role":"authenticated"}', true);
select ok(
  (select sum(billed_usd) from public.get_usage_summary(now() - interval '1 day', now())) >= 0.0216,
  'the owner sees their billed usage');
reset role;

-- ── Config sync ───────────────────────────────────────────────────────

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d1","role":"authenticated"}', true);
select is(
  public.put_user_config('app', 0, '{"settings.json":"e30="}'::jsonb),
  1::bigint, 'the first config write is version 1');
select throws_ok(
  $$ select public.put_user_config('app', 0, '{"settings.json":"e30="}'::jsonb) $$,
  'P0001', 'version_conflict',
  'a write against a stale version is refused');
select is(
  (public.get_user_config('app')->>'version')::bigint,
  1::bigint, 'the latest config is readable by its owner');
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d2","role":"authenticated"}', true);
select is(public.get_user_config('app'), null, 'another user cannot read it');
reset role;

select * from finish();
rollback;
