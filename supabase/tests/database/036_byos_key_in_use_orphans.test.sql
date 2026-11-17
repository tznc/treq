-- BYOS: a key cannot be removed while a dead placement still holds a
-- machine the orphan sweep must delete with it.
begin;
select plan(2);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000035b1', 'chaos-a@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_chaos_a', 'cus_chaos_a', 'user', '00000000-0000-0000-0000-0000000035b1', 'pro', 'active', now());

select public.provider_credential_save('00000000-0000-0000-0000-0000000035b1', 'boat', 'tok-old', null);

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status, tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000350a', '00000000-0000-0000-0000-0000000035b1',
   'boat', 'boat-chaos', 'us_east', 'small', 'provisioning', 's', 'boat:small');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, provider_resource_id,
   state, role, credential_source, credential_id)
select '00000000-0000-0000-0000-00000000351a', '00000000-0000-0000-0000-00000000350a',
       '00000000-0000-0000-0000-0000000035b1', 'boat', 'boat:small', 'us_east', 'boat-chaos',
       'stopped', 'dead', 'byos', c.id
  from public.remote_provider_credentials c
 where c.owner_user_id = '00000000-0000-0000-0000-0000000035b1';

-- A claimed retiring placement or a failed destroy is dead with its machine
-- still up. Without the key the orphan sweep can never delete it.
set local role authenticated;
set local request.jwt.claim.sub = '00000000-0000-0000-0000-0000000035b1';
select throws_ok(
  $$ select public.delete_provider_credential('boat') $$,
  'P0001', 'credential_in_use', 'a key cannot be removed while a dead placement still holds a machine');
reset role;

update public.remote_instance_placements set state = 'deleted'
 where id = '00000000-0000-0000-0000-00000000351a';
set local role authenticated;
select lives_ok(
  $$ select public.delete_provider_credential('boat') $$,
  'the key can be removed once that machine is deleted');
reset role;

select * from finish();
rollback;
