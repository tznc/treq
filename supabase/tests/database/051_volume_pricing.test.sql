-- Volume pricing (072): the markup falls with usage in the billing period.
-- One tier is 730 hours of a Medium (4 vCPU) sandbox = 10,512,000 vCPU-seconds.
-- Managed markup 20% / 18% / 16%, BYOS fee 5% / 4% / 3%, graduated.
begin;
select plan(14);

-- ── Tier rate ──────────────────────────────────────────────────────────────
select is(treq_internal.volume_rate_bps('managed', 2000, 0, 0), 2000::numeric,
  'no usage yet: full managed markup');
select is(treq_internal.volume_rate_bps('managed', 2000, 0, 10512000), 2000::numeric,
  'first sandbox-month: 20%');
select is(treq_internal.volume_rate_bps('managed', 2000, 10512000, 10512000), 1800::numeric,
  'second sandbox-month: 18%');
select is(treq_internal.volume_rate_bps('managed', 2000, 21024000, 105120000), 1600::numeric,
  'past two sandbox-months: 16%');
select is(treq_internal.volume_rate_bps('managed', 2000, 5256000, 10512000), 1900::numeric,
  'a slice across a boundary blends the two tiers');
select is(treq_internal.volume_rate_bps('byos', 500, 0, 3600), 500::numeric,
  'BYOS first sandbox-month: 5%');
select is(treq_internal.volume_rate_bps('byos', 500, 10512000, 3600), 400::numeric,
  'BYOS second sandbox-month: 4%');
select is(treq_internal.volume_rate_bps('byos', 500, 30000000, 3600), 300::numeric,
  'BYOS past two sandbox-months: 3%');

-- ── build_usage_reports applies the tier ─────────────────────────────────
update public.machine_type_prices set effective_from = now() - interval '30 days' where effective_to is null;

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000051a1', 'volume-heavy@example.com'),
  ('00000000-0000-0000-0000-0000000051a2', 'volume-light@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status,
   current_period_start, current_period_end, stripe_event_created)
values ('sub_volume_heavy', 'cus_volume_heavy', 'user', '00000000-0000-0000-0000-0000000051a1', 'pro', 'active',
        now() - interval '2 days', now() + interval '28 days', now());

-- Two sandbox-months of managed usage already reported this period.
insert into public.usage_reports
  (identifier, owner_user_id, hour, seconds, provider_cost_nanousd, billed_nanousd, managed_vcpu_seconds, sent_at)
values ('volume-heavy-prior', '00000000-0000-0000-0000-0000000051a1',
        date_trunc('hour', now() - interval '1 day'), 3600, 0, 0, 21024000, now());
-- The same usage before this period does not count.
insert into public.usage_reports
  (identifier, owner_user_id, hour, seconds, provider_cost_nanousd, billed_nanousd, managed_vcpu_seconds, sent_at)
values ('volume-light-old', '00000000-0000-0000-0000-0000000051a2',
        date_trunc('month', now()) - interval '40 days', 3600, 0, 0, 99999999, now());

insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000051b1', '00000000-0000-0000-0000-0000000051a1', 'e2b', 'us_east', 'small', 'provisioning', 's', 'e2b:s'),
       ('00000000-0000-0000-0000-0000000051b2', '00000000-0000-0000-0000-0000000051a2', 'e2b', 'us_east', 'small', 'provisioning', 's', 'e2b:s');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000051c1', '00000000-0000-0000-0000-0000000051b1', '00000000-0000-0000-0000-0000000051a1', 'e2b', 'e2b:s', 'us_east', 'provisioning', 'active'),
       ('00000000-0000-0000-0000-0000000051c2', '00000000-0000-0000-0000-0000000051b2', '00000000-0000-0000-0000-0000000051a2', 'e2b', 'e2b:s', 'us_east', 'provisioning', 'active');
select public.set_placement_state('00000000-0000-0000-0000-0000000051c1', 'ready', null, now() - interval '3 hours');
select public.set_placement_state('00000000-0000-0000-0000-0000000051c2', 'ready', null, now() - interval '3 hours');

select ok(public.build_usage_reports(now()) >= 2, 'builds the new hours');

select ok((select bool_and(abs(managed_vcpu_seconds - seconds * 2) <= 1) from public.usage_reports
            where owner_user_id = '00000000-0000-0000-0000-0000000051a2'
              and identifier <> 'volume-light-old'),
  'records vCPU-seconds (e2b:s has 2 vCPU)');
select ok((select bool_and(abs(billed_nanousd - round(provider_cost_nanousd * 1.20)) <= 1)
             from public.usage_reports
            where owner_user_id = '00000000-0000-0000-0000-0000000051a2'
              and identifier <> 'volume-light-old'),
  'light user, old usage outside the period: 20% markup');
select ok((select bool_and(abs(billed_nanousd - round(provider_cost_nanousd * 1.16)) <= 1)
             from public.usage_reports
            where owner_user_id = '00000000-0000-0000-0000-0000000051a1'
              and identifier <> 'volume-heavy-prior'),
  'heavy user past two sandbox-months this period: 16% markup');
select is(public.build_usage_reports(now()), 0, 'a second build writes nothing');
select ok(not has_function_privilege('authenticated', 'treq_internal.volume_rate_bps(text, integer, numeric, numeric)', 'execute'),
  'clients cannot call the tier function');

select * from finish();
rollback;
