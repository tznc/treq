-- Provider catalog rows, admitted kinds and statuses (038_sandbox_catalog.sql).
begin;
select plan(25);

select is(
  (select array_agg(billing_granularity_sec order by kind) from public.sandbox_providers
    where kind in ('daytona', 'e2b', 'fly_machines')),
  array[1, 1, 1],
  'daytona, e2b and fly_machines bill per second');

select ok(
  (select pg_get_constraintdef(oid) from pg_constraint where conname = 'remote_instances_provider_kind_check')
    ~ 'daytona' and
  (select pg_get_constraintdef(oid) from pg_constraint where conname = 'remote_instances_provider_kind_check')
    ~ 'fly_machines' and
  (select pg_get_constraintdef(oid) from pg_constraint where conname = 'remote_instances_provider_kind_check')
    ~ 'e2b',
  'remote_instances admits daytona, fly_machines and e2b');

select throws_ok(
  $$insert into public.sandbox_providers (kind, display_name, billing_granularity_sec)
    values ('nope', 'Nope', 1)$$,
  '23514', null,
  'the provider kind check rejects unknown kinds');
select throws_ok(
  $$insert into public.sandbox_providers (kind, display_name, billing_granularity_sec)
    values ('modal', 'Modal test', 1)$$,
  '23514', null,
  'modal is out of scope and not an admitted kind');
select throws_ok(
  $$insert into public.sandbox_providers (kind, display_name, billing_granularity_sec)
    values ('fly_sprites', 'Fly Sprites', 3600)$$,
  '23514', null,
  'fly_sprites is not an admitted kind');

-- ── Daytona ─────────────────────────────────────────────────────────────

select is(
  (select count(*)::int from public.provider_machine_types where provider_kind = 'daytona'),
  4,
  'daytona has a machine type per size tier');
select is(
  (select count(*)::int
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
    where t.provider_kind = 'daytona'
      and t.vcpu = st.vcpu and t.ram_gib = st.ram_gib and t.disk_gb = st.min_disk_gb
      and t.vendor_type = st.id
      and t.regions @> array['us_east', 'us_west', 'eu_west', 'ap_southeast']),
  4,
  'daytona machine types match the size tiers in all four regions');
select is(
  (select array_agg(p.running_nanousd_per_sec order by st.sort)
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
     join public.machine_type_prices p on p.machine_type_id = t.id and p.effective_to is null
    where t.provider_kind = 'daytona'),
  array[46000, 92000, 184000, 368000]::bigint[],
  'daytona running prices follow $0.0504/vCPU-h + $0.0162/GiB-h');
select is(
  (select count(*)::int
     from public.machine_type_prices p
     join public.provider_machine_types t on t.id = p.machine_type_id
    where t.provider_kind = 'daytona' and p.stopped_nanousd_per_gb_sec = 30),
  4,
  'daytona bills stopped storage at $0.000108/GiB-h');
select ok(
  not exists (
    select 1 from public.pro_provider_defaults d
     join public.provider_machine_types t on t.id = d.machine_type_id
    where t.provider_kind = 'daytona'),
  'Boat stays the cheapest default; daytona is never picked');

-- ── Fly Machines ────────────────────────────────────────────────────────

select is(
  (select count(*)::int
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
    where t.provider_kind = 'fly_machines'
      and t.vcpu = st.vcpu and t.ram_gib = st.ram_gib and t.disk_gb = st.min_disk_gb
      and t.id = 'fly_machines:' || st.id
      and t.regions @> array['us_east', 'us_west', 'eu_west', 'ap_southeast']),
  4,
  'fly_machines machine types match the size tiers in all four regions');
select is(
  (select array_agg(t.vendor_type order by st.sort)
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
    where t.provider_kind = 'fly_machines'),
  array['shared-cpu-2x:4096', 'performance-4x:8192', 'performance-8x:16384', 'performance-16x:32768'],
  'vendor types name the Fly guest');
select is(
  (select array_agg(p.running_nanousd_per_sec order by st.sort)
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
     join public.machine_type_prices p on p.machine_type_id = t.id and p.effective_to is null
    where t.provider_kind = 'fly_machines'),
  array[9661, 50228, 100457, 200913]::bigint[],
  'fly_machines running prices follow docs.fly.io/about/pricing');
select is(
  (select count(*)::int
     from public.machine_type_prices p
     join public.provider_machine_types t on t.id = p.machine_type_id
    where t.provider_kind = 'fly_machines' and p.stopped_nanousd_per_gb_sec = 57),
  4,
  'fly_machines bills stopped volumes at $0.15/GB-month');
-- All customers would share one Fly app and its private network.
select is(
  (select count(*)::int from public.provider_machine_types
    where provider_kind = 'fly_machines' and selectable),
  0,
  'fly_machines machine types are hidden from customers');
select ok(
  not exists (
    select 1 from public.pro_provider_defaults d
     join public.provider_machine_types t on t.id = d.machine_type_id
    where t.provider_kind = 'fly_machines'),
  'fly_machines is never a default');

-- ── E2B ─────────────────────────────────────────────────────────────────

select is(
  (select count(*)::int
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
    where t.provider_kind = 'e2b'
      and t.vcpu = st.vcpu and t.ram_gib = st.ram_gib and t.disk_gb = st.min_disk_gb
      and t.id = 'e2b:' || st.id
      and t.vendor_type = st.id
      and t.regions @> array['us_east', 'us_west', 'eu_west', 'ap_southeast']),
  4,
  'e2b machine types match the size tiers in all four regions');
select is(
  (select array_agg(p.running_nanousd_per_sec order by st.sort)
     from public.provider_machine_types t
     join public.size_tiers st on st.id = t.tier_id
     join public.machine_type_prices p on p.machine_type_id = t.id and p.effective_to is null
    where t.provider_kind = 'e2b'),
  array[46000, 92000, 184000, 368000]::bigint[],
  'e2b running prices follow $0.000014/vCPU-s + $0.0000045/GiB-s');
select is(
  (select count(*)::int from public.provider_machine_types where provider_kind = 'e2b' and selectable),
  4,
  'e2b machine types are selectable');
select ok(
  not exists (
    select 1 from public.pro_provider_defaults d
     join public.provider_machine_types t on t.id = d.machine_type_id
    where t.provider_kind = 'e2b'),
  'Boat stays the cheapest default; e2b is never picked');

-- E2B and Daytona cost the same in every tier; the tie breaks by id.
update public.sandbox_providers set status = 'draining' where kind = 'boat';
set local role service_role;
select public.recompute_pro_defaults();
reset role;
select is(
  (select machine_type_id from public.pro_provider_defaults where tier_id = 's' and region = 'us_east'),
  'daytona:s',
  'with Boat draining, the Daytona/E2B price tie goes to daytona:s by id');

-- ── Provider status ─────────────────────────────────────────────────────

update public.sandbox_providers set status = 'retiring' where kind = 'local';
select is((select status from public.sandbox_providers where kind = 'local'), 'retiring',
  'a provider can be retiring');
select throws_ok($$update public.sandbox_providers set status = 'gone' where kind = 'local'$$,
  '23514', null, 'other provider statuses are rejected');
select is(
  (select count(*)::int from public.pro_provider_defaults d
     join public.provider_machine_types t on t.id = d.machine_type_id
    where t.provider_kind = 'local'),
  0,
  'a retiring provider is never a default');

-- pg-safeupdate (PostgREST) rejects a DELETE with no WHERE.
select ok(
  pg_get_functiondef('public.recompute_pro_defaults()'::regprocedure) ~ 'delete from public\.pro_provider_defaults\s+where',
  'recompute_pro_defaults deletes with a WHERE clause, so it runs through PostgREST');

select * from finish();
rollback;
