-- E2B-pegged pricing (072): running time bills at E2B's rate for the size,
-- whichever provider runs it. Managed adds the markup (20%); BYOS charges the
-- BYOS fee (5%) of that rate. Stopped disk bills at Daytona's storage rate
-- on every provider that keeps a stopped disk, with the same markup and fee. provider_cost_nanousd stays the real provider cost.
begin;
select plan(19);

update public.machine_type_prices set effective_from = now() - interval '30 days' where effective_to is null;

-- ── List rate ──────────────────────────────────────────────────────────────
select is(treq_internal.list_running_nanousd_per_sec('boat:default', now(), 1), 92000::bigint,
  'Boat Medium lists at E2B Medium');
select is(treq_internal.list_running_nanousd_per_sec('e2b:m', now(), 1), 92000::bigint,
  'E2B lists at its own rate');
select is(treq_internal.list_running_nanousd_per_sec('cloudflare:s', now(), 1), 46000::bigint,
  'Cloudflare Small lists at E2B Small');
select is(treq_internal.list_running_nanousd_per_sec('fly_machines:xl', now(), 1), 368000::bigint,
  'Fly XL lists at E2B XL');

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

-- Spend cap estimate before anything is reported: list rate, not Boat's.
select ok(abs(treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000051a1', now())
              - 3 * 3600 * 92000 * 1.2) < 92000 * 1.2 * 5,
  'the spend cap counts open managed usage at E2B Medium + 20%');

select ok(public.build_usage_reports(now()) >= 2, 'builds the hours');

select ok((select bool_and(abs(provider_cost_nanousd - seconds * 10000) <= 10000)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a1'),
  'provider cost stays Boat''s real rate');
select ok((select bool_and(abs(billed_nanousd - round(seconds * 92000 * 1.2)) <= 92000 * 1.2)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a1'),
  'managed Boat Medium bills at E2B Medium + 20%');
select ok((select bool_and(abs(byos_billed_nanousd - round(seconds * 92000 * 0.05)) <= 92000 * 0.05
                           and billed_nanousd = byos_billed_nanousd)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a2'),
  'BYOS on Boat pays 5% of E2B Medium');

-- Stopped disk on Daytona itself (30 nano-USD/GB-s).
insert into auth.users (id, email) values ('00000000-0000-0000-0000-0000000051a3', 'pegged-disk@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000051b3', '00000000-0000-0000-0000-0000000051a3', 'daytona', 'us_east', 'small', 'provisioning', 's', 'daytona:s');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000051c3', '00000000-0000-0000-0000-0000000051b3', '00000000-0000-0000-0000-0000000051a3',
        'daytona', 'daytona:s', 'us_east', 'provisioning', 'active');
select public.set_placement_state('00000000-0000-0000-0000-0000000051c3', 'suspended', 10, now() - interval '3 hours');
select public.build_usage_reports(now());
select ok((select bool_and(abs(billed_nanousd - round(seconds * 30 * 10 * 1.2)) <= 30 * 10 * 1.2)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a3'),
  'stopped disk bills at the provider''s storage rate + 20%');

-- ── Stopped disk pegged to Daytona ─────────────────────────────────────────
select is(treq_internal.list_stopped_nanousd_per_gb_sec('boat:default', now(), 0), 30::bigint,
  'Boat stopped disk lists at Daytona''s storage rate');
select is(treq_internal.list_stopped_nanousd_per_gb_sec('e2b:xl', now(), 0), 30::bigint,
  'E2B stopped disk lists at Daytona''s storage rate');
select is(treq_internal.list_stopped_nanousd_per_gb_sec('cloudflare:s', now(), 0), 0::bigint,
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
select ok((select bool_and(abs(billed_nanousd - round(seconds * 30 * 10 * 1.2)) <= 30 * 10 * 1.2
                           and provider_cost_nanousd = 0)
             from public.usage_reports where owner_user_id = '00000000-0000-0000-0000-0000000051a4'),
  'managed Boat stopped disk bills Daytona''s rate + 20%; Boat charges us nothing');

-- BYOS on Boat: suspend the running workspace from above.
select public.set_placement_state('00000000-0000-0000-0000-0000000051c2', 'suspended', 10, null);
select ok(abs(treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000051a2', now() + interval '1 hour')
              - treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000051a2', now())
              - 3600 * 30 * 10 * 0.05) < 1000,
  'BYOS stopped disk on Boat bills 5% of Daytona''s storage rate');

-- ── Catalogs ─────────────────────────────────────────────────────────────
select is((select count(distinct (r->>'hourly_usd')::numeric)::int
             from jsonb_array_elements(public.get_machine_catalog()) t,
                  jsonb_array_elements(t->'regions') r
            where t->>'id' = 'm'), 1, 'Medium has one price in every region');
select is((select (r->>'hourly_usd')::numeric
             from jsonb_array_elements(public.get_machine_catalog()) t,
                  jsonb_array_elements(t->'regions') r
            where t->>'id' = 'm' limit 1), 0.39744, 'Medium is E2B Medium + 20% (US$0.39744/h)');

set local role authenticated;
set local request.jwt.claims = '{"sub":"00000000-0000-0000-0000-0000000051a2","role":"authenticated"}';
select is((select (e->>'hourly_usd')::numeric from jsonb_array_elements(public.get_byos_catalog()) e
            where e->>'tier_id' = 'm'), 0.01656, 'the BYOS catalog shows 5% of E2B Medium');
reset role;

select * from finish();
rollback;
