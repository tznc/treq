-- Cloudflare provider rows (070_cloudflare_provider.sql).
begin;
select plan(5);

select is(
  (select billing_granularity_sec from public.sandbox_providers where kind = 'cloudflare'),
  1,
  'cloudflare bills per second');
select ok(
  pg_get_constraintdef((select oid from pg_constraint where conname = 'remote_instances_provider_kind_check'))
    ~ 'cloudflare',
  'remote_instances admits cloudflare');
select is(
  (select array_agg(id order by id) from public.provider_machine_types where provider_kind = 'cloudflare'),
  array['cloudflare:m', 'cloudflare:s'],
  'cloudflare has only s and m');
select is(
  (select count(*)::int from public.provider_machine_types where provider_kind = 'cloudflare' and selectable),
  0,
  'cloudflare is BYOS only');
select is(
  (select array_agg(p.running_nanousd_per_sec order by t.id desc)
     from public.provider_machine_types t
     join public.machine_type_prices p on p.machine_type_id = t.id and p.effective_to is null
    where t.provider_kind = 'cloudflare'),
  array[61120, 111400]::bigint[],
  'cloudflare prices follow $0.00002/vCPU-s + $0.0000025/GiB-s + $0.00000007/GB-s');

select * from finish();
rollback;
