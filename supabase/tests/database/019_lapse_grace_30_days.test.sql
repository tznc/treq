-- A lapsed cloud workspace is kept 30 days before the sweep may delete it
-- (046_lapse_grace_30_days.sql).
begin;
select plan(3);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000003a1', 'lapse-a@example.com'),
  ('00000000-0000-0000-0000-0000000003a2', 'lapse-b@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, lapsed_at) values
  ('10000000-0000-0000-0000-0000000003a1', '00000000-0000-0000-0000-0000000003a1', 'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '8 days'),
  ('10000000-0000-0000-0000-0000000003a2', '00000000-0000-0000-0000-0000000003a2', 'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '31 days');

select is(public.remote_claim_lapsed_deletion('10000000-0000-0000-0000-0000000003a1'), false,
  'an instance lapsed 8 days ago is not claimed for deletion');
select is(public.remote_claim_lapsed_deletion('10000000-0000-0000-0000-0000000003a2'), true,
  'an instance lapsed 31 days ago is claimed for deletion');
select ok(col_description('public.remote_instances'::regclass,
  (select attnum from pg_attribute where attrelid = 'public.remote_instances'::regclass and attname = 'lapsed_at')) like '%30 days%',
  'lapsed_at documents the 30-day window');

select * from finish();
rollback;
