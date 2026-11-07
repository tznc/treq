-- Fixed list prices (073): one price per size on every provider, stored in
-- treq_internal.admin_config under 'list_prices' so they are edited without a
-- migration. Managed and BYOS each have their own hourly rate;
-- stopped disk bills per GB-month wherever the provider keeps the disk.
-- provider_cost_nanousd stays the real provider cost.
--
-- Medium: managed US$0.40/h = 111,111.1 nano-USD/s; BYOS US$0.017/h = 4,722.2.
-- Disk: managed US$0.10/GB-month = 38.05 nano-USD/GB-s; BYOS US$0.004 = 1.52.
begin;
select plan(27);

update public.machine_type_prices set effective_from = now() - interval '30 days' where effective_to is null;

-- ── List prices ────────────────────────────────────────────────────────────
select is((select value from treq_internal.admin_config where key = 'list_prices'),
  '{"tiers": {"s":  {"managed_usd_per_hour": 0.20, "byos_usd_per_hour": 0.0085},
              "m":  {"managed_usd_per_hour": 0.40, "byos_usd_per_hour": 0.017},
              "l":  {"managed_usd_per_hour": 0.80, "byos_usd_per_hour": 0.034},
              "xl": {"managed_usd_per_hour": 1.60, "byos_usd_per_hour": 0.068}},
    "stopped_disk": {"managed_usd_per_gb_month": 0.10, "byos_usd_per_gb_month": 0.004}}'::jsonb,
  'one rounded price per size, in admin_config');
select throws_ok(
  $$ update treq_internal.admin_config set value = value #- '{tiers,xl}' where key = 'list_prices' $$,
  'P0001', 'admin_config: list_prices does not match its schema',
  'an edit that drops a size is rejected');
select throws_ok(
  $$ update treq_internal.admin_config set value = jsonb_set(value, '{stopped_disk,byos_usd_per_gb_month}', '"cheap"') where key = 'list_prices' $$,
  'P0001', 'admin_config: list_prices does not match its schema',
  'an edit with a non-number price is rejected');
select throws_ok(
  $$ update treq_internal.admin_config set value = jsonb_set(value, '{tiers,s,managed_usd_per_hour}', '-1') where key = 'list_prices' $$,
  'P0001', 'admin_config: list_prices does not match its schema',
  'a negative price is rejected');
select ok(not has_schema_privilege('authenticated', 'treq_internal', 'usage')
          and not has_schema_privilege('anon', 'treq_internal', 'usage'),
  'clients cannot reach the treq_internal schema');
select is(public.admin_config_get('list_prices') #>> '{tiers,m,byos_usd_per_hour}', '0.017',
  'service code reads a key through admin_config_get');
select ok(not has_function_privilege('authenticated', 'public.admin_config_get(text)', 'execute')
          and not has_function_privilege('authenticated', 'public.admin_config_set(text, jsonb)', 'execute'),
  'signed-in users cannot call the admin_config RPCs');
savepoint rpc_set;
select public.admin_config_set('list_prices',
  jsonb_set(public.admin_config_get('list_prices'), '{tiers,s,managed_usd_per_hour}', '0.25'));
select is(public.admin_config_get('list_prices') #>> '{tiers,s,managed_usd_per_hour}', '0.25',
  'service code writes a key through admin_config_set');
rollback to savepoint rpc_set;
select is(round(treq_internal.billed_running_nanousd_per_sec('boat:default', 'managed', 0), 3), 111111.111,
  'Boat Medium bills the managed Medium price');
select is(round(treq_internal.billed_running_nanousd_per_sec('e2b:m', 'byos', 0), 3), 4722.222,
  'E2B Medium on a BYOS key bills the BYOS Medium price');
select is(round(treq_internal.billed_running_nanousd_per_sec('fly_machines:xl', 'managed', 0), 3), 444444.444,
  'Fly XL bills the managed XL price');
savepoint price_edit;
update treq_internal.admin_config set value = jsonb_set(value, '{tiers,m,managed_usd_per_hour}', '0.36') where key = 'list_prices';
select is(round(treq_internal.billed_running_nanousd_per_sec('boat:default', 'managed', 0), 3), 100000.000,
  'editing the config row changes the price, no migration needed');
rollback to savepoint price_edit;

-- ── Usage reports ────────────────────────────────────────────────────────
insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000051a1', 'pegged-managed@example.com'),
  ('00000000-0000-0000-0000-0000000051a2', 'pegged-byos@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values ('sub_pegged_byos', 'cus_pegged_byos', 'user', '00000000-0000-0000-0000-0000000051a2', 'pro', 'active', now());
select public.provider_credential_save('00000000-0000-0000-0000-0000000051a2', 'boat', 'tok-pegged', null);

insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000051b1', '00000000-0000-0000-0000-0000000051a1', 'boat', 'eu_west', 'medium', 'provisioning', 'm', 'boat:default'),
       ('00000000-0000-0000-0000-0000000051b2', '00000000-0000-0000-0000-0000000051a2', 'boat', 'eu_west', 'medium', 'provisioning', 'm', 'boat:default');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, credential_source, credential_id)
values ('00000000-0000-0000-0000-0000000051c1', '00000000-0000-0000-0000-0000000051b1', '00000000-0000-0000-0000-0000000051a1',
        'boat', 'boat:default', 'eu_west', 'provisioning', 'active', 'managed', null);
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, credential_source, credential_id)
select '00000000-0000-0000-0000-0000000051c2', '00000000-0000-0000-0000-0000000051b2', '00000000-0000-0000-0000-0000000051a2',
       'boat', 'boat:default', 'eu_west', 'provisioning', 'active', 'byos', c.id
  from public.remote_provider_credentials c where c.owner_user_id = '00000000-0000-0000-0000-0000000051a2';
select public.set_placement_state('00000000-0000-0000-0000-0000000051c1', 'ready', null, now() - interval '3 hours');
select public.set_placement_state('00000000-0000-0000-0000-0000000051c2', 'ready', null, now() - interval '3 hours');

-- Spend cap estimate before anything is reported: list price, not Boat's cost.
select ok(abs(treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000051a1', now())
              - 3 * 3600 * 111111.111) < 111111.111 * 5,
  'the spend cap counts open managed usage at the managed Medium price');

select ok(public.build_usage_reports(now()) >= 2, 'builds the hours');

select ok((select bool_and(abs(provider_cost_nanousd - seconds * 10000) <= 10000)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a1'),
  'provider cost stays Boat''s real rate');
select ok((select bool_and(abs(billed_nanousd - round(seconds * 111111.111)) <= 111111.111)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a1'),
  'managed Boat Medium bills US$0.40/h');
select ok((select bool_and(abs(byos_billed_nanousd - round(seconds * 4722.222)) <= 4722.222
                           and billed_nanousd = byos_billed_nanousd)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a2'),
  'BYOS on Boat bills US$0.017/h');

-- Stopped disk on Daytona: 10 GB at US$0.10/GB-month.
insert into auth.users (id, email) values ('00000000-0000-0000-0000-0000000051a3', 'pegged-disk@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000051b3', '00000000-0000-0000-0000-0000000051a3', 'daytona', 'us_east', 'small', 'provisioning', 's', 'daytona:s');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000051c3', '00000000-0000-0000-0000-0000000051b3', '00000000-0000-0000-0000-0000000051a3',
        'daytona', 'daytona:s', 'us_east', 'provisioning', 'active');
select public.set_placement_state('00000000-0000-0000-0000-0000000051c3', 'suspended', 10, now() - interval '3 hours');
select public.build_usage_reports(now());
select ok((select bool_and(abs(billed_nanousd - round(seconds * 380.518)) <= 380.518)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a3'),
  'stopped disk bills US$0.10/GB-month');

-- ── Stopped disk on every provider that keeps it ───────────────────────────
select is(round(treq_internal.billed_stopped_nanousd_per_gb_sec('boat:default', 'managed', 0), 4), 38.0518,
  'Boat stopped disk bills US$0.10/GB-month');
select is(round(treq_internal.billed_stopped_nanousd_per_gb_sec('e2b:xl', 'byos', 0), 4), 1.5221,
  'E2B stopped disk on a BYOS key bills US$0.004/GB-month');
select is(treq_internal.billed_stopped_nanousd_per_gb_sec('cloudflare:s', 'managed', 99), 0::numeric,
  'Cloudflare loses its disk when stopped, so nothing bills');

insert into auth.users (id, email) values ('00000000-0000-0000-0000-0000000051a4', 'pegged-boat-disk@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000051b4', '00000000-0000-0000-0000-0000000051a4', 'boat', 'eu_west', 'medium', 'provisioning', 'm', 'boat:default');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000051c4', '00000000-0000-0000-0000-0000000051b4', '00000000-0000-0000-0000-0000000051a4',
        'boat', 'boat:default', 'eu_west', 'provisioning', 'active');
select public.set_placement_state('00000000-0000-0000-0000-0000000051c4', 'suspended', 10, now() - interval '3 hours');
select is((select count(*)::int from public.usage_intervals
            where placement_id = '00000000-0000-0000-0000-0000000051c4' and kind = 'stopped_storage'), 1,
  'a suspended Boat workspace opens a stopped-disk interval');
select public.build_usage_reports(now());
select ok((select bool_and(abs(billed_nanousd - round(seconds * 380.518)) <= 380.518
                           and provider_cost_nanousd = 0)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a4'),
  'managed Boat stopped disk bills US$0.10/GB-month; Boat charges us nothing');

-- BYOS on Boat: suspend the running workspace from above.
select public.set_placement_state('00000000-0000-0000-0000-0000000051c2', 'suspended', 10, null);
select ok(abs(treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000051a2', now() + interval '1 hour')
              - treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000051a2', now())
              - 3600 * 15.221) < 1000,
  'BYOS stopped disk on Boat bills US$0.004/GB-month');

-- ── Catalogs ─────────────────────────────────────────────────────────────
select is((select count(distinct (r->>'hourly_usd')::numeric)::int
             from jsonb_array_elements(public.get_machine_catalog()) t,
                  jsonb_array_elements(t->'regions') r
            where t->>'id' = 'm'), 1, 'Medium has one price in every region');
select is((select (r->>'hourly_usd')::numeric
             from jsonb_array_elements(public.get_machine_catalog()) t,
                  jsonb_array_elements(t->'regions') r
            where t->>'id' = 'm' limit 1), 0.40, 'Medium is US$0.40/h');

set local role authenticated;
set local request.jwt.claims = '{"sub":"00000000-0000-0000-0000-0000000051a2","role":"authenticated"}';
select is((select (e->>'hourly_usd')::numeric from jsonb_array_elements(public.get_byos_catalog()) e
            where e->>'tier_id' = 'm'), 0.017, 'the BYOS catalog shows US$0.017/h for Medium');
reset role;

select * from finish();
rollback;
