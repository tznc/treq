-- Boat is the Pro default in every tier and region (079_boat_pro_default.sql).
begin;
select plan(4);

set local role service_role;
select public.recompute_pro_defaults();
reset role;

select is(
  (select count(*)::int from public.pro_provider_defaults d
     join public.provider_machine_types t on t.id = d.machine_type_id
    where t.provider_kind <> 'boat'),
  0,
  'every Pro default is a Boat machine type');

select is(
  (select machine_type_id from public.pro_provider_defaults where tier_id = 's' and region = 'us_east'),
  'boat:small',
  'us_east gets Boat although Boat lists only eu_west');

select is(
  (select count(*)::int from public.pro_provider_defaults),
  (select count(*)::int from public.size_tiers) * 4,
  'every tier has a default in all four regions');

-- An operator draining Boat falls back to the cheapest admitted type.
update public.sandbox_providers set status = 'draining' where kind = 'boat';
set local role service_role;
select public.recompute_pro_defaults();
reset role;
select is(
  (select count(*)::int from public.pro_provider_defaults d
     join public.provider_machine_types t on t.id = d.machine_type_id
    where t.provider_kind = 'boat'),
  0,
  'a draining Boat is never a default');

select * from finish();
