-- Team cloud (065): an organization owner chooses whether members may run
-- treq-managed sandboxes, BYOS on the organization's own keys, or both, and
-- sets an organization-wide and a per-member spend cap. A member picks the
-- profile (personal or an organization) their workspace bills to. Usage on
-- an organization profile is attributed to that organization.
begin;
select plan(40);

update public.machine_type_prices set effective_from = now() - interval '30 days' where effective_to is null;

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000041a1', 'tc-owner@example.com'),
  ('00000000-0000-0000-0000-0000000041a2', 'tc-member@example.com'),
  ('00000000-0000-0000-0000-0000000041a3', 'tc-outsider@example.com'),
  ('00000000-0000-0000-0000-0000000041a4', 'tc-pro-member@example.com');
insert into public.organizations (id, name) values ('00000000-0000-0000-0000-0000000041f1', 'Team Cloud');
insert into public.organization_members (org_id, user_id, role) values
  ('00000000-0000-0000-0000-0000000041f1', '00000000-0000-0000-0000-0000000041a1', 'owner'),
  ('00000000-0000-0000-0000-0000000041f1', '00000000-0000-0000-0000-0000000041a2', 'member'),
  ('00000000-0000-0000-0000-0000000041f1', '00000000-0000-0000-0000-0000000041a4', 'member');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, current_period_end, stripe_event_created)
values
  ('sub_tc_team', 'cus_tc_team', 'organization', '00000000-0000-0000-0000-0000000041f1', 'team', 'active', now() + interval '30 days', now()),
  ('sub_tc_pro', 'cus_tc_pro', 'user', '00000000-0000-0000-0000-0000000041a4', 'pro', 'active', now() + interval '30 days', now());

-- ── Settings: off until the owner turns cloud on ─────────────────────────
select is(treq_internal.cloud_access('00000000-0000-0000-0000-0000000041a2', '00000000-0000-0000-0000-0000000041f1'),
  '{"entitled": false, "allow_byos": false, "allow_managed": false}'::jsonb,
  'an organization starts with cloud off for members');

set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a2","role":"authenticated"}', true);
select throws_ok($$ select public.set_organization_cloud_settings('00000000-0000-0000-0000-0000000041f1', true, true, 500, 50) $$,
  'P0001', 'not_organization_owner', 'a member cannot change cloud settings');
select is((public.get_organization_cloud_settings('00000000-0000-0000-0000-0000000041f1'))->>'allow_managed', 'false',
  'a member can read the settings');

select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a1","role":"authenticated"}', true);
select throws_ok($$ select public.set_organization_cloud_settings('00000000-0000-0000-0000-0000000041f1', true, true, 4, 50) $$,
  'P0001', 'spend_cap_too_low', 'the organization cap has the same floor');
select throws_ok($$ select public.set_organization_cloud_settings('00000000-0000-0000-0000-0000000041f1', true, true, 500, 4) $$,
  'P0001', 'spend_cap_too_low', 'the member cap has the same floor');
select throws_ok($$ select public.set_organization_cloud_settings('00000000-0000-0000-0000-0000000041f1', true, true, 1e9, 50) $$,
  'P0001', 'spend_cap_too_high', 'the organization cap has a ceiling');
select is((public.set_organization_cloud_settings('00000000-0000-0000-0000-0000000041f1', true, false, 5, 5))->>'allow_managed', 'true',
  'the owner turns on managed sandboxes');
select results_eq(
  $$ select org_id, name, role, has_team, allow_managed, allow_byos, org_cap_usd, member_cap_usd
       from public.list_my_cloud_organizations() $$,
  $$ values ('00000000-0000-0000-0000-0000000041f1'::uuid, 'Team Cloud'::text, 'owner'::text, true, true, false, 5.00::numeric, 5.00::numeric) $$,
  'the desktop lists the caller''s organizations with role, Team and cloud settings');
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a3","role":"authenticated"}', true);
select is((select count(*)::int from public.list_my_cloud_organizations()), 0, 'a non-member lists nothing');
reset role;

select is(treq_internal.cloud_access('00000000-0000-0000-0000-0000000041a2', '00000000-0000-0000-0000-0000000041f1'),
  '{"entitled": true, "allow_byos": false, "allow_managed": true}'::jsonb,
  'members get what the owner allowed');
select is((treq_internal.cloud_access('00000000-0000-0000-0000-0000000041a3', '00000000-0000-0000-0000-0000000041f1'))->>'entitled', 'false',
  'a non-member gets nothing from the organization');
select is((treq_internal.cloud_access('00000000-0000-0000-0000-0000000041a2', null))->>'entitled', 'false',
  'a Team member without personal Pro has no personal cloud');
select is(treq_internal.cloud_access('00000000-0000-0000-0000-0000000041a4', null),
  '{"entitled": true, "allow_byos": true, "allow_managed": true}'::jsonb,
  'personal Pro has managed and BYOS');

-- ── Profiles ──────────────────────────────────────────────────────────────
set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a3","role":"authenticated"}', true);
select throws_ok($$ select public.set_cloud_profile('00000000-0000-0000-0000-0000000041f1') $$,
  'P0001', 'not_organization_member', 'only a member can bill to an organization');
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a2","role":"authenticated"}', true);
select is((public.set_cloud_profile('00000000-0000-0000-0000-0000000041f1'))->>'org_id', '00000000-0000-0000-0000-0000000041f1',
  'a member switches to the organization profile');
reset role;

select is(treq_internal.user_billing_org('00000000-0000-0000-0000-0000000041a2'), '00000000-0000-0000-0000-0000000041f1'::uuid,
  'with no workspace the profile decides the billing organization');
select is(treq_internal.user_billing_org('00000000-0000-0000-0000-0000000041a4'), null,
  'the default profile is personal');
select is((public.cloud_context_for('00000000-0000-0000-0000-0000000041a2'))->>'entitled', 'true',
  'the member is entitled through the organization');
select is(public.has_cloud('00000000-0000-0000-0000-0000000041a2'), true, 'has_cloud follows the profile');

-- ── Organization keys ─────────────────────────────────────────────────────
select throws_ok($$ select public.provider_credential_save_org('00000000-0000-0000-0000-0000000041f1',
    '00000000-0000-0000-0000-0000000041a2', 'boat', 'org-key', null) $$,
  'P0001', 'not_organization_owner', 'only an owner saves an organization key');
select lives_ok($$ select public.provider_credential_save_org('00000000-0000-0000-0000-0000000041f1',
    '00000000-0000-0000-0000-0000000041a1', 'boat', 'org-key', 'Org Boat') $$,
  'an owner saves an organization key');
select lives_ok($$ select public.provider_credential_save('00000000-0000-0000-0000-0000000041a4', 'boat', 'personal-key', null) $$,
  'a personal key for the same kind still saves');
select is((select count(*)::int from public.remote_provider_credentials
            where provider_kind = 'boat'
              and (owner_org_id = '00000000-0000-0000-0000-0000000041f1' or owner_user_id = '00000000-0000-0000-0000-0000000041a4')),
  2, 'the organization key and the personal key are separate rows');
select is((select owner_user_id from public.remote_provider_credentials where owner_org_id = '00000000-0000-0000-0000-0000000041f1'),
  null, 'an organization key belongs to no user, so it outlives whoever saved it');

set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a2","role":"authenticated"}', true);
select is((select count(*)::int from public.list_org_provider_credentials('00000000-0000-0000-0000-0000000041f1')), 1,
  'a member sees the organization''s key metadata');
select is((select count(*)::int from public.list_provider_credentials()), 0,
  'the organization key is not the member''s personal key');
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a3","role":"authenticated"}', true);
select is((select count(*)::int from public.list_org_provider_credentials('00000000-0000-0000-0000-0000000041f1')), 0,
  'a non-member sees nothing');
reset role;

-- ── Billing owner on workspaces ───────────────────────────────────────────
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id, billing_org_id)
values ('00000000-0000-0000-0000-0000000041b2', '00000000-0000-0000-0000-0000000041a2', 'boat', 'eu_west', 'small',
        'provisioning', 's', 'boat:small', '00000000-0000-0000-0000-0000000041f1');
select lives_ok($$
  insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, credential_source, credential_id)
  select '00000000-0000-0000-0000-0000000041c2', '00000000-0000-0000-0000-0000000041b2', '00000000-0000-0000-0000-0000000041a2',
         'boat', 'boat:small', 'eu_west', 'provisioning', 'active', 'byos', c.id
    from public.remote_provider_credentials c
   where c.owner_org_id = '00000000-0000-0000-0000-0000000041f1' and c.provider_kind = 'boat' $$,
  'an organization workspace runs on the organization''s key');
select is((select billing_org_id from public.remote_instance_placements where id = '00000000-0000-0000-0000-0000000041c2'),
  '00000000-0000-0000-0000-0000000041f1'::uuid, 'a placement bills to its instance''s organization');
select throws_ok($$
  insert into public.remote_instance_placements (instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, credential_source, credential_id)
  select '00000000-0000-0000-0000-0000000041b2', '00000000-0000-0000-0000-0000000041a2',
         'boat', 'boat:small', 'eu_west', 'provisioning', 'candidate', 'byos', c.id
    from public.remote_provider_credentials c
   where c.owner_user_id = '00000000-0000-0000-0000-0000000041a4' and c.owner_org_id is null $$,
  'P0001', 'byos_credential_mismatch', 'an organization workspace cannot use someone''s personal key');

update public.remote_instances set active_placement_id = '00000000-0000-0000-0000-0000000041c2'
 where id = '00000000-0000-0000-0000-0000000041b2';
-- At a BYOS list price of $1.80/h for Small, 3 hours bill about $5.40, over the $5 member cap.
update treq_internal.admin_config set value = jsonb_set(value, '{tiers,s,byos_usd_per_hour}', '1.80') where key = 'list_prices';
select public.set_placement_state('00000000-0000-0000-0000-0000000041c2', 'ready', null, now() - interval '3 hours');
select is((select billing_org_id from public.usage_intervals where placement_id = '00000000-0000-0000-0000-0000000041c2' limit 1),
  '00000000-0000-0000-0000-0000000041f1'::uuid, 'usage records the organization it bills to');
select ok(public.build_usage_reports(now()) > 0, 'reports build');
select ok(exists (select 1 from public.usage_reports
                   where owner_user_id = '00000000-0000-0000-0000-0000000041a2'
                     and billing_org_id = '00000000-0000-0000-0000-0000000041f1'
                     and identifier like '%00000000-0000-0000-0000-0000000041f1%'),
  'an organization''s usage report names the organization');
select ok(public.build_final_usage_reports('00000000-0000-0000-0000-0000000041a2', now()) > 0,
  'the flush before an account is deleted adds the current partial hour');
select ok(exists (select 1 from public.usage_reports
                       where owner_user_id = '00000000-0000-0000-0000-0000000041a2'
                         and billing_org_id = '00000000-0000-0000-0000-0000000041f1'
                         and hour = date_trunc('hour', now())),
  'and bills that hour to the organization too');

-- ── Spend: organization-wide and per member ───────────────────────────────
select is((treq_internal.cloud_spend_status('00000000-0000-0000-0000-0000000041a2', now()))->>'scope', 'organization',
  'a member''s spend is measured in the organization');
select is((treq_internal.cloud_spend_status('00000000-0000-0000-0000-0000000041a2', now()))->>'state', 'capped',
  '3 hours at 5% of $36/h reaches the $5 member cap');
select ok(exists (select 1 from public.spend_cap_candidates(now()) where user_id = '00000000-0000-0000-0000-0000000041a2'),
  'the enforcement sweep finds the capped member');

-- ── Lapse follows the organization's settings ─────────────────────────────
update public.organization_cloud_settings set allow_managed = false, allow_byos = false
 where org_id = '00000000-0000-0000-0000-0000000041f1';
select ok(exists (select 1 from public.remote_lapse_candidates() where instance_id = '00000000-0000-0000-0000-0000000041b2'),
  'turning cloud off for the organization lapses its workspaces');

set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000041a1","role":"authenticated"}', true);
select throws_ok($$ select public.delete_org_provider_credential('00000000-0000-0000-0000-0000000041f1', 'boat') $$,
  'P0001', 'credential_in_use', 'an organization key in use cannot be removed');
reset role;

select * from finish();
rollback;
