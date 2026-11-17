-- Per-placement integration secrets (078_integration_secrets.sql) live in
-- Vault, only service_role can use them, and they go with their placement.
begin;
select plan(6);

select function_privs_are('public', 'integration_secret_load', array['uuid'], 'authenticated',
  array[]::text[], 'authenticated cannot load integration secrets');
select function_privs_are('public', 'integration_secret_save', array['uuid', 'text'], 'authenticated',
  array[]::text[], 'authenticated cannot save integration secrets');
select table_privs_are('public', 'remote_integration_secrets', 'authenticated', array[]::text[],
  'authenticated cannot read the secrets table');

insert into auth.users (id, email) values ('00000000-0000-0000-0000-0000000000b1', 'int@example.com');
insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-00000000b0a1', '00000000-0000-0000-0000-0000000000b1',
  'local', 'dev-int', 'us_east', 'small', 'ready', 's', 'local:s');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, provider_resource_id, state, role)
values ('00000000-0000-0000-0000-00000000b0b1', '00000000-0000-0000-0000-00000000b0a1',
  '00000000-0000-0000-0000-0000000000b1', 'local', 'local:s', 'us_east', 'dev-int', 'ready', 'active');

set local role service_role;
select public.integration_secret_save('00000000-0000-0000-0000-00000000b0b1', 'first');
select public.integration_secret_save('00000000-0000-0000-0000-00000000b0b1', 'second');
select is(public.integration_secret_load('00000000-0000-0000-0000-00000000b0b1'), 'second',
  'a later save replaces the secret');

reset role;
create temp table ids as select secret_id from public.remote_integration_secrets;
delete from public.remote_instance_placements where id = '00000000-0000-0000-0000-00000000b0b1';
select is((select count(*)::int from public.remote_integration_secrets), 0,
  'the secret goes with its placement');
select is((select count(*)::int from vault.secrets where id in (select secret_id from ids)), 0,
  'and so does its Vault secret');

select * from finish();
rollback;
