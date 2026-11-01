-- Bring your own sandbox (037, 038): provider credentials in Vault, the
-- gate, and 5% billing.
begin;
select plan(26);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000b1', 'byos-pro@example.com'),
  ('00000000-0000-0000-0000-0000000000b2', 'byos-free@example.com'),
  ('00000000-0000-0000-0000-0000000000b3', 'byos-other@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_byos_pro', 'cus_byos_pro', 'user', '00000000-0000-0000-0000-0000000000b1', 'pro', 'active', now()),
  ('sub_byos_other', 'cus_byos_other', 'user', '00000000-0000-0000-0000-0000000000b3', 'pro', 'active', now());

-- ── The gate ────────────────────────────────────────────────────────────

-- The backend is always on; only the desktop feature preview hides BYOS.
select is(treq_internal.user_can_byos('00000000-0000-0000-0000-0000000000b1'), true,
  'a Pro user can bring their own sandbox with no server flag');
select is(treq_internal.user_can_byos('00000000-0000-0000-0000-0000000000b2'), false,
  'a Free user cannot');
select ok(not has_function_privilege('authenticated', 'treq_internal.user_can_byos(uuid)', 'execute'),
  'signed-in users cannot call the internal gate');

-- ── Credentials ─────────────────────────────────────────────────────────

select throws_ok(
  $$ select public.provider_credential_save('00000000-0000-0000-0000-0000000000b2', 'boat', 'tok-free', null) $$,
  'P0001', 'byos_not_allowed', 'saving a credential needs the gate');
select lives_ok(
  $$ select public.provider_credential_save('00000000-0000-0000-0000-0000000000b1', 'boat', 'tok-boat-1', 'My Boat') $$,
  'service code saves a validated credential');
select public.provider_credential_save('00000000-0000-0000-0000-0000000000b1', 'boat', 'tok-boat-2', 'My Boat');
select is((select count(*)::int from public.remote_provider_credentials
            where owner_user_id = '00000000-0000-0000-0000-0000000000b1' and provider_kind = 'boat'), 1,
  'a credential is unique per owner and provider kind: saving again replaces it');
select throws_ok(
  $$ insert into public.remote_provider_credentials (owner_user_id, provider_kind, vault_secret_id)
     values ('00000000-0000-0000-0000-0000000000b1', 'boat', gen_random_uuid()) $$,
  '23505', null, 'the table refuses a second credential for the same kind');
select throws_ok(
  $$ select public.provider_credential_save('00000000-0000-0000-0000-0000000000b1', 'local', 'x', null) $$,
  'P0001', 'byos_kind_unsupported', 'the local stub takes no credential');
select hasnt_column('public', 'remote_provider_credentials', 'secret', 'no plaintext secret column');
select public.provider_credential_save('00000000-0000-0000-0000-0000000000b3', 'daytona', 'tok-other', null);

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000b1","role":"authenticated"}', true);
select results_eq(
  $$ select provider_kind, label, validated_at is not null from public.list_provider_credentials() $$,
  $$ values ('boat'::text, 'My Boat'::text, true) $$,
  'the owner lists their own credential metadata only');
select ok(
  not exists (select 1 from public.list_provider_credentials() c where c::text like '%tok-%'),
  'listing never returns the secret');
select throws_ok($$ select vault_secret_id from public.remote_provider_credentials $$, '42501', null,
  'the Vault id column is not readable');
select is((select count(*)::int from public.remote_provider_credentials), 1,
  'RLS shows the owner only their own rows');
select throws_ok(
  $$ select public.provider_credential_load_for_placement('00000000-0000-0000-0000-00000000b101') $$,
  '42501', null, 'signed-in users cannot load a decrypted key');
reset role;

-- ── Placements and billing ──────────────────────────────────────────────

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status, tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000b001', '00000000-0000-0000-0000-0000000000b1',
   'boat', 'boat-b1', 'us_east', 'small', 'provisioning', 's', 'boat:small');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, provider_resource_id,
   state, role, credential_source, credential_id)
select '00000000-0000-0000-0000-00000000b101', '00000000-0000-0000-0000-00000000b001',
       '00000000-0000-0000-0000-0000000000b1', 'boat', 'boat:small', 'us_east', 'boat-b1',
       'provisioning', 'active', 'byos', c.id
  from public.remote_provider_credentials c
 where c.owner_user_id = '00000000-0000-0000-0000-0000000000b1' and c.provider_kind = 'boat';

select is(
  public.provider_credential_load_for_placement('00000000-0000-0000-0000-00000000b101')->>'key',
  'tok-boat-2', 'service code loads the BYOS placement''s decrypted key');

select throws_ok(
  $$ insert into public.remote_instance_placements
       (instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
     values ('00000000-0000-0000-0000-00000000b001', '00000000-0000-0000-0000-0000000000b1',
             'daytona', 'daytona:s', 'us_east', 'provisioning', 'candidate') $$,
  'P0001', 'byos_placement_must_stay_byos',
  'a BYOS workspace cannot gain a treq-managed placement');
select throws_ok(
  $$ insert into public.remote_instance_placements
       (instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, credential_source, credential_id)
     select '00000000-0000-0000-0000-00000000b001', '00000000-0000-0000-0000-0000000000b1',
            'daytona', 'daytona:s', 'us_east', 'provisioning', 'candidate', 'byos', c.id
       from public.remote_provider_credentials c
      where c.owner_user_id = '00000000-0000-0000-0000-0000000000b3' $$,
  'P0001', 'byos_credential_mismatch',
  'a placement cannot use another user''s credential');

select public.set_placement_state('00000000-0000-0000-0000-00000000b101', 'ready');
select is((select billing_mode from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-00000000b101' and ended_at is null),
  'byos', 'a BYOS placement opens its interval in byos billing mode');
select is((select credential_source from public.remote_instances
            where id = '00000000-0000-0000-0000-00000000b001'), 'byos',
  'the instance mirrors its active placement''s credential source');

-- Same owner-hour: one hour on a managed boat:small (5000 nUSD/s) and one
-- on a BYOS boat:small. Managed bills 18e6 x 1.2; BYOS bills 18e6 x 0.05.
delete from public.usage_intervals where owner_user_id = '00000000-0000-0000-0000-0000000000b1';
insert into public.remote_instances
  (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-00000000b002', '00000000-0000-0000-0000-0000000000b1',
        'boat', 'us_east', 'small', 'deleted', 's', 'boat:small');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-00000000b102', '00000000-0000-0000-0000-00000000b002',
        '00000000-0000-0000-0000-0000000000b1', 'boat', 'boat:small', 'us_east', 'deleted', 'dead');
insert into public.usage_intervals (placement_id, owner_user_id, kind, price_id, started_at, ended_at, billing_mode)
select pl.id, '00000000-0000-0000-0000-0000000000b1', 'running', p.id,
       date_trunc('hour', now()) - interval '2 hours', date_trunc('hour', now()) - interval '1 hour', pl.mode
  from public.machine_type_prices p,
       (values ('00000000-0000-0000-0000-00000000b101'::uuid, 'byos'),
               ('00000000-0000-0000-0000-00000000b102'::uuid, 'managed')) pl(id, mode)
 where p.machine_type_id = 'boat:small' and p.effective_to is null;

select throws_ok(
  $$ update public.usage_intervals set billing_mode = 'managed' where billing_mode = 'byos' $$,
  'P0001', 'usage_intervals is append and close only', 'billing mode is immutable');

select is(treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000000b1',
            date_trunc('hour', now()) - interval '1 hour'),
  case when date_trunc('hour', now()) - interval '2 hours' >= date_trunc('month', now(), 'UTC')
       then (21600000 + 900000)::bigint else null end,
  'the spend cap counts managed at cost x 1.2 plus BYOS at cost x 0.05 (unreported)');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select public.build_usage_reports(date_trunc('hour', now()));
reset role;
select results_eq(
  $$ select provider_cost_nanousd, billed_nanousd, byos_billed_nanousd from public.usage_reports
      where owner_user_id = '00000000-0000-0000-0000-0000000000b1'
        and hour = date_trunc('hour', now()) - interval '2 hours' $$,
  $$ values (36000000::bigint, 22500000::bigint, 900000::bigint) $$,
  'one report per owner-hour: managed at 20% markup plus BYOS at 5% of cost');
select is(treq_internal.byos_markup_bps(), 500, 'the BYOS rate is 500 bps by default');

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000b1","role":"authenticated"}', true);
select results_eq(
  $$ select billed_usd, byos_billed_usd from public.get_usage_summary(now() - interval '3 hours', now())
      where hour = date_trunc('hour', now()) - interval '2 hours' $$,
  $$ values (0.0225::numeric, 0.0009::numeric) $$,
  'the usage summary shows the total and the BYOS part');
select throws_ok($$ select public.delete_provider_credential('boat') $$, 'P0001', 'credential_in_use',
  'a credential a live workspace uses cannot be removed');
reset role;

update public.remote_instance_placements set role = 'dead', state = 'deleted'
 where id = '00000000-0000-0000-0000-00000000b101';
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000b1","role":"authenticated"}', true);
select public.delete_provider_credential('boat');
reset role;
select is(
  (select count(*)::int from vault.secrets where name like 'treq-byos:00000000-0000-0000-0000-0000000000b1:%'),
  0, 'removing a credential deletes its Vault secret');

select * from finish();
rollback;
