-- Boat is the Pro default in every tier and region. Boat ignores the region
-- code, so it backs regions it does not list too; other providers are only a
-- fallback while Boat is draining, retiring or down.
create or replace function public.recompute_pro_defaults()
returns integer
language plpgsql
security definer
set search_path to ''
as $function$
declare
  v_count int;
begin
  perform treq_internal.require_service_role();
  -- pg-safeupdate (loaded for PostgREST) rejects a DELETE with no WHERE.
  delete from public.pro_provider_defaults where true;
  with admitted as (
    select t.*, p.running_nanousd_per_sec
      from public.provider_machine_types t
      join public.sandbox_providers sp on sp.kind = t.provider_kind
      join public.machine_type_prices p
        on p.id = treq_internal.current_price_id(t.id, now())
     where t.selectable
       and sp.status = 'active'
       and sp.health <> 'down'
  ), regions as (
    select distinct r.region from admitted a cross join lateral unnest(a.regions) as r(region)
  )
  insert into public.pro_provider_defaults (tier_id, region, machine_type_id)
  select distinct on (a.tier_id, r.region) a.tier_id, r.region, a.id
    from admitted a
    join regions r on r.region = any(a.regions) or a.provider_kind = 'boat'
   order by a.tier_id, r.region, a.provider_kind <> 'boat', a.running_nanousd_per_sec, a.id;
  get diagnostics v_count = row_count;
  return v_count;
end;
$function$;

select public.recompute_pro_defaults();
