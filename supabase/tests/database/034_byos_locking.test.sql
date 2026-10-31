-- BYOS guard and locking (037, 038): the guard on insert and update, no
-- BYOS placement without a credential, and the credential row locked by
-- delete and by the placement paths.
begin;
select plan(9);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000029b1', 'sec-a@example.com'),
  ('00000000-0000-0000-0000-0000000029b2', 'sec-b@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_sec_a', 'cus_sec_a', 'user', '00000000-0000-0000-0000-0000000029b1', 'pro', 'active', now()),
  ('sub_sec_b', 'cus_sec_b', 'user', '00000000-0000-0000-0000-0000000029b2', 'pro', 'active', now());
select public.provider_credential_save('00000000-0000-0000-0000-0000000029b1', 'boat', 'tok-a-boat', null);
select public.provider_credential_save('00000000-0000-0000-0000-0000000029b1', 'daytona', 'tok-a-day', null);
select public.provider_credential_save('00000000-0000-0000-0000-0000000029b2', 'boat', 'tok-b-boat', null);

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status, tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000290a', '00000000-0000-0000-0000-0000000029b1',
   'boat', 'boat-sec', 'us_east', 'small', 'provisioning', 's', 'boat:small');

select throws_ok(
  $$ insert into public.remote_instance_placements
       (instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, credential_source)
     values ('00000000-0000-0000-0000-00000000290a', '00000000-0000-0000-0000-0000000029b1',
             'boat', 'boat:small', 'us_east', 'provisioning', 'standby', 'byos') $$,
  'P0001', 'byos_credential_required', 'a BYOS placement cannot be inserted without a credential');

insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, provider_resource_id,
   state, role, credential_source, credential_id)
select '00000000-0000-0000-0000-00000000291a', '00000000-0000-0000-0000-00000000290a',
       '00000000-0000-0000-0000-0000000029b1', 'boat', 'boat:small', 'us_east', 'boat-sec',
       'provisioning', 'active', 'byos', c.id
  from public.remote_provider_credentials c
 where c.owner_user_id = '00000000-0000-0000-0000-0000000029b1' and c.provider_kind = 'boat';

select throws_ok(
  $$ update public.remote_instance_placements
        set credential_id = (select id from public.remote_provider_credentials
                              where owner_user_id = '00000000-0000-0000-0000-0000000029b2')
      where id = '00000000-0000-0000-0000-00000000291a' $$,
  'P0001', 'byos_credential_mismatch', 'an update cannot point a placement at another user''s credential');
select throws_ok(
  $$ update public.remote_instance_placements
        set credential_id = (select id from public.remote_provider_credentials
                              where owner_user_id = '00000000-0000-0000-0000-0000000029b1'
                                and provider_kind = 'daytona')
      where id = '00000000-0000-0000-0000-00000000291a' $$,
  'P0001', 'byos_credential_mismatch', 'an update cannot point a placement at a credential of another kind');
select throws_ok(
  $$ update public.remote_instance_placements
        set credential_source = 'managed', credential_id = null
      where id = '00000000-0000-0000-0000-00000000291a' $$,
  'P0001', 'byos_placement_must_stay_byos', 'an update cannot turn the active BYOS placement managed');
select lives_ok(
  $$ update public.remote_instance_placements set state = 'ready'
      where id = '00000000-0000-0000-0000-00000000291a' $$,
  'other updates are untouched');

select ok(
  pg_get_functiondef('public.delete_provider_credential(text)'::regprocedure) ~* 'for update',
  'deleting a credential locks its row before checking placements');
select ok(
  pg_get_functiondef('treq_internal.placements_byos_guard()'::regprocedure) ~* 'for (key )?share',
  'the placement guard locks the credential it checks');
select is(
  public.provider_credential_load_for_placement('00000000-0000-0000-0000-00000000291a')->>'key',
  'tok-a-boat', 'loading a placement''s key still works');
select ok(
  pg_get_functiondef('public.provider_credential_load_for_placement(uuid)'::regprocedure) ~* 'for share',
  'loading a placement''s key locks the credential row');

select * from finish();
rollback;
