-- Per-placement treq-gateway secrets live in Vault (045_gateway_secrets.sql).
begin;
select plan(9);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000e2', 'gateway@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000000f2', '00000000-0000-0000-0000-0000000000e2',
        'boat', 'us_east', 'small', 'ready', 's', 'boat:small');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000000a2', '00000000-0000-0000-0000-0000000000f2',
        '00000000-0000-0000-0000-0000000000e2', 'boat', 'boat:small', 'us_east', 'ready', 'active');

select has_table('public', 'remote_gateway_secrets', 'remote_gateway_secrets exists');
select has_column('public', 'remote_gateway_secrets', 'secret_id', 'it stores only a Vault secret id');
select hasnt_column('public', 'remote_gateway_secrets', 'secret', 'it has no plaintext secret column');

select lives_ok(
  $$select public.gateway_secret_save('00000000-0000-0000-0000-0000000000a2', 'first')$$,
  'service code can save a secret');
select is(public.gateway_secret_load('00000000-0000-0000-0000-0000000000a2'), 'first',
  'the saved secret loads back decrypted');

select public.gateway_secret_save('00000000-0000-0000-0000-0000000000a2', 'second');
select is(public.gateway_secret_load('00000000-0000-0000-0000-0000000000a2'), 'second',
  'saving again replaces the secret');

select is(public.gateway_secret_load('00000000-0000-0000-0000-0000000000ff'), null,
  'an unknown placement has no secret');

select ok(
  not has_function_privilege('authenticated', 'public.gateway_secret_load(uuid)', 'execute'),
  'signed-in users cannot read gateway secrets');

delete from public.remote_instance_placements where id = '00000000-0000-0000-0000-0000000000a2';
select is(
  (select count(*)::int from vault.secrets where name = 'treq-gateway:00000000-0000-0000-0000-0000000000a2'),
  0,
  'deleting the placement deletes its Vault secret');

select * from finish();
rollback;
