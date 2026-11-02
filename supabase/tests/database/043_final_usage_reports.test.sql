-- Final usage for an account being deleted (060): every hour up to now,
-- including the current partial hour, for that owner only.
begin;
select plan(6);

-- Seed prices start at db reset; let them cover the backdated usage below.
update public.machine_type_prices set effective_from = now() - interval '30 days' where effective_to is null;

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000040a1', 'final-usage@example.com'),
  ('00000000-0000-0000-0000-0000000040a2', 'final-usage-other@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000040b1', '00000000-0000-0000-0000-0000000040a1', 'e2b', 'us_east', 'small', 'provisioning', 's', 'e2b:s'),
       ('00000000-0000-0000-0000-0000000040b2', '00000000-0000-0000-0000-0000000040a2', 'e2b', 'us_east', 'small', 'provisioning', 's', 'e2b:s');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000040c1', '00000000-0000-0000-0000-0000000040b1', '00000000-0000-0000-0000-0000000040a1', 'e2b', 'e2b:s', 'us_east', 'provisioning', 'active'),
       ('00000000-0000-0000-0000-0000000040c2', '00000000-0000-0000-0000-0000000040b2', '00000000-0000-0000-0000-0000000040a2', 'e2b', 'e2b:s', 'us_east', 'provisioning', 'active');
select public.set_placement_state('00000000-0000-0000-0000-0000000040c1', 'ready', null, now() - interval '90 minutes');
select public.set_placement_state('00000000-0000-0000-0000-0000000040c2', 'ready', null, now() - interval '90 minutes');

select ok(public.build_final_usage_reports('00000000-0000-0000-0000-0000000040a1', now()) >= 2,
  'builds the owner''s hours up to now');
select ok(exists (select 1 from public.usage_reports
                   where owner_user_id = '00000000-0000-0000-0000-0000000040a1' and hour = date_trunc('hour', now())),
  'includes the current partial hour');
select cmp_ok((select sum(seconds) from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000040a1'),
  '>=', 5399::bigint, 'counts all 90 minutes');
select is((select count(*)::int from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000040a2'),
  0, 'builds nothing for another owner');
select is(public.build_final_usage_reports('00000000-0000-0000-0000-0000000040a1', now()), 0,
  'a second call writes nothing');
select ok(not has_function_privilege('authenticated', 'public.build_final_usage_reports(uuid, timestamptz)', 'execute'),
  'only the service role can build final usage');

select * from finish();
rollback;
