-- Server-side Pro enforcement from 027_billing_enforcement.sql: the
-- entitlement check for callers without a user JWT, the installation owner
-- check the merge queue uses, the merge queue opt-in gate, and the database
-- side of the cloud workspace lapse sweep.
begin;
select plan(58);

-- Fixtures, created as postgres. d1 has Pro, d2 is Free, d3 had Pro and
-- canceled it, d4 and d5 subscribe later in the test.
insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000d1', 'enforce-pro@example.com'),
  ('00000000-0000-0000-0000-0000000000d2', 'enforce-free@example.com'),
  ('00000000-0000-0000-0000-0000000000d3', 'enforce-lapsed@example.com'),
  ('00000000-0000-0000-0000-0000000000d4', 'enforce-resubscribed@example.com'),
  ('00000000-0000-0000-0000-0000000000d5', 'enforce-grace@example.com');

insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_enf_pro', 'cus_enf_pro', 'user', '00000000-0000-0000-0000-0000000000d1', 'pro', 'active', now()),
  ('sub_enf_lapsed', 'cus_enf_lapsed', 'user', '00000000-0000-0000-0000-0000000000d3', 'pro', 'active', now());

insert into public.github_app_installations (id, account_login, account_type, app_id, linked_user_id) values
  (9001, 'enforce-pro', 'User', 1, '00000000-0000-0000-0000-0000000000d1'),
  (9002, 'enforce-free', 'User', 1, '00000000-0000-0000-0000-0000000000d2'),
  (9003, 'enforce-lapsed', 'User', 1, '00000000-0000-0000-0000-0000000000d3'),
  (9004, 'enforce-unlinked', 'User', 1, null);

insert into public.github_repositories (id, installation_id, owner, name, full_name) values
  (9101, 9001, 'enforce-pro', 'repo', 'enforce-pro/repo'),
  (9102, 9002, 'enforce-free', 'repo', 'enforce-free/repo'),
  (9103, 9003, 'enforce-lapsed', 'repo', 'enforce-lapsed/repo');

-- d3 turned the queue on while Pro, then canceled.
insert into public.merge_queue_configs (repo_id, target_branch, enabled) values (9103, 'main', true);
update public.billing_subscriptions set status = 'canceled' where stripe_subscription_id = 'sub_enf_lapsed';

-- ── Shape and grants ──────────────────────────────────────────────────────
select is_definer('public', 'installation_has_pro', array['bigint'],
  'installation_has_pro is security definer');
select function_privs_are('public', 'installation_has_pro', array['bigint'], 'anon', array[]::text[],
  'anon cannot execute installation_has_pro');
select function_privs_are('public', 'installation_has_pro', array['bigint'], 'authenticated', array[]::text[],
  'authenticated cannot execute installation_has_pro');
select function_privs_are('public', 'installation_has_pro', array['bigint'], 'service_role', array['EXECUTE'],
  'service_role can execute installation_has_pro');
select function_privs_are('treq_internal', 'user_has_pro', array['uuid'], 'anon', array[]::text[],
  'anon cannot execute treq_internal.user_has_pro');
select function_privs_are('treq_internal', 'user_has_pro', array['uuid'], 'authenticated', array[]::text[],
  'authenticated cannot execute treq_internal.user_has_pro');
select function_privs_are('public', 'remote_lapse_candidates', array[]::text[], 'authenticated', array[]::text[],
  'authenticated cannot list lapse candidates');
select function_privs_are('public', 'remote_lapse_candidates', array[]::text[], 'service_role', array['EXECUTE'],
  'service_role can list lapse candidates');
select function_privs_are('public', 'remote_sync_instance_lapse', array['uuid'], 'authenticated', array[]::text[],
  'authenticated cannot stamp or clear a lapse');
select function_privs_are('public', 'remote_sync_instance_lapse', array['uuid'], 'service_role', array['EXECUTE'],
  'service_role can stamp or clear a lapse');
select function_privs_are('public', 'remote_claim_lapsed_deletion', array['uuid'], 'authenticated', array[]::text[],
  'authenticated cannot claim an instance for deletion');
select function_privs_are('public', 'remote_claim_lapsed_deletion', array['uuid'], 'service_role', array['EXECUTE'],
  'service_role can claim an instance for deletion');
select has_column('public', 'remote_instances', 'lapsed_at', 'remote_instances records when entitlement lapsed');

-- ── Entitlement without a user JWT ────────────────────────────────────────
-- pg_cron and other jobs run as postgres with no JWT claims.
select set_config('request.jwt.claims', '', true);
select is(treq_internal.user_has_pro('00000000-0000-0000-0000-0000000000d1'), true,
  'the internal check answers for a Pro user without a JWT');
select is(treq_internal.user_has_pro('00000000-0000-0000-0000-0000000000d3'), false,
  'the internal check answers false once Pro is canceled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000d1'), false,
  'has_pro still refuses to answer about a user to a caller that is neither that user nor the service role');

-- ── installation_has_pro, as the service role ─────────────────────────────
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(public.installation_has_pro(9001), true, 'an installation linked by a Pro user is entitled');
select is(public.installation_has_pro(9002), false, 'an installation linked by a Free user is not entitled');
select is(public.installation_has_pro(9003), false, 'an installation whose owner canceled Pro is not entitled');
select is(public.installation_has_pro(9004), false, 'an installation nobody linked is not entitled');
select is(public.installation_has_pro(424242), false, 'an unknown installation is not entitled');
select is(public.has_pro('00000000-0000-0000-0000-0000000000d1'), true,
  'has_pro keeps answering the service role about any user');

-- ── A signed-in user learns nothing new ───────────────────────────────────
reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d2","role":"authenticated"}', true);

select throws_ok($$select public.installation_has_pro(9001)$$, '42501', null,
  'a user cannot ask whether another installation is entitled');
select throws_ok($$select treq_internal.user_has_pro('00000000-0000-0000-0000-0000000000d1')$$, '42501', null,
  'a user cannot call the internal check');
select throws_ok($$select * from public.remote_lapse_candidates()$$, '42501', null,
  'a user cannot list lapsed instances');
select is(public.has_pro('00000000-0000-0000-0000-0000000000d1'), false,
  'has_pro still answers false about another user');

-- ── set_merge_queue_enabled and direct config writes ──────────────────────
-- Free user d2.
select throws_ok($$select public.set_merge_queue_enabled('enforce-free/repo', true)$$,
  'PT402', 'The merge queue needs Pro',
  'a Free user cannot turn the merge queue on');
select is((select count(*)::int from public.merge_queue_configs where repo_id = 9102), 0,
  'the refused call wrote no config row');
select is(public.set_merge_queue_enabled('enforce-free/repo', false), false,
  'a Free user can turn the merge queue off');
select throws_ok(
  $$update public.merge_queue_configs set enabled = true where repo_id = 9102$$,
  'PT402', 'The merge queue needs Pro',
  'a Free user cannot turn the queue on by updating the config row');
select throws_ok(
  $$insert into public.merge_queue_configs (repo_id, target_branch, enabled) values (9102, 'release', true)$$,
  'PT402', 'The merge queue needs Pro',
  'a Free user cannot turn the queue on by inserting a config row');
select throws_ok($$select public.set_merge_queue_enabled('enforce-pro/repo', true)$$,
  'P0002', null,
  'a user still cannot touch a repo they did not link, so the Pro check reveals nothing about it');
-- Inserting a config for someone else's repo fails the same way whether
-- that owner pays or not.
select throws_ok(
  $$insert into public.merge_queue_configs (repo_id, target_branch, enabled) values (9101, 'leak', true)$$,
  '42501', null,
  'a config insert for a Pro owner''s repo is refused by RLS');
select throws_ok(
  $$insert into public.merge_queue_configs (repo_id, target_branch, enabled) values (9103, 'leak', true)$$,
  '42501', null,
  'a config insert for a Free owner''s repo is refused the same way, so it reveals no entitlement');

-- Lapsed user d3, whose queue was on.
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d3","role":"authenticated"}', true);
select throws_ok($$select public.set_merge_queue_enabled('enforce-lapsed/repo', true)$$,
  'PT402', 'The merge queue needs Pro',
  'a user whose Pro ended cannot turn the queue on again');
select lives_ok(
  $$update public.merge_queue_configs set batch_size = 2 where repo_id = 9103$$,
  'a lapsed user can still edit settings of a queue that is already on');
select throws_ok(
  $$update public.merge_queue_configs set target_branch = 'release' where repo_id = 9103$$,
  'PT402', 'The merge queue needs Pro',
  'moving an enabled queue to another branch counts as turning that one on');
select lives_ok(
  $$insert into public.merge_queue_configs (repo_id, target_branch, enabled, batch_size)
    values (9103, 'main', true, 3)
    on conflict (repo_id, target_branch) do update set batch_size = excluded.batch_size, enabled = excluded.enabled$$,
  'an upsert that keeps an enabled queue enabled is not a new opt-in');
select is(public.set_merge_queue_enabled('enforce-lapsed/repo', false), false,
  'a lapsed user can turn the queue off');

-- Pro user d1.
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000d1","role":"authenticated"}', true);
select is(public.set_merge_queue_enabled('enforce-pro/repo', true), true,
  'a Pro user can turn the merge queue on');
select results_eq(
  $$select enabled from public.merge_queue_configs where repo_id = 9101 and target_branch = 'main'$$,
  $$values (true)$$,
  'the Pro user''s config row is enabled');
select lives_ok(
  $$insert into public.merge_queue_configs (repo_id, target_branch, enabled) values (9101, 'release', true)$$,
  'a Pro user can write an enabled config row directly');

reset role;

-- ── Cloud workspace lapse ─────────────────────────────────────────────────
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, lapsed_at) values
  ('00000000-0000-0000-0000-00000000e001', '00000000-0000-0000-0000-0000000000d1', 'fly_sprites', 'us_east', 'small', 'ready', null),
  ('00000000-0000-0000-0000-00000000e002', '00000000-0000-0000-0000-0000000000d2', 'fly_sprites', 'us_east', 'small', 'ready', null),
  ('00000000-0000-0000-0000-00000000e003', '00000000-0000-0000-0000-0000000000d3', 'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '31 days'),
  ('00000000-0000-0000-0000-00000000e004', '00000000-0000-0000-0000-0000000000d4', 'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '40 days'),
  ('00000000-0000-0000-0000-00000000e005', '00000000-0000-0000-0000-0000000000d5', 'fly_sprites', 'us_east', 'small', 'suspended', now() - interval '29 days');

-- d4 subscribes again. The webhook's write clears the lapse at once, so a
-- sweep that has not run since cannot count the old lapse.
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_enf_back', 'cus_enf_back', 'user', '00000000-0000-0000-0000-0000000000d4', 'pro', 'trialing', now());
select is((select lapsed_at from public.remote_instances where id = '00000000-0000-0000-0000-00000000e004'), null,
  'a new Pro subscription clears the owner''s lapse');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select results_eq(
  $$select instance_id, owner_has_pro from public.remote_lapse_candidates() order by instance_id$$,
  $$values
    ('00000000-0000-0000-0000-00000000e002'::uuid, false),
    ('00000000-0000-0000-0000-00000000e003'::uuid, false),
    ('00000000-0000-0000-0000-00000000e005'::uuid, false)$$,
  'candidates are the live instances of owners without Pro');

select isnt(public.remote_sync_instance_lapse('00000000-0000-0000-0000-00000000e002'), null,
  'a Free owner''s instance is stamped as lapsed');
select is(
  public.remote_sync_instance_lapse('00000000-0000-0000-0000-00000000e002'),
  (select lapsed_at from public.remote_instances where id = '00000000-0000-0000-0000-00000000e002'),
  'stamping again keeps the first lapse time');
select is(public.remote_sync_instance_lapse('00000000-0000-0000-0000-00000000e001'), null,
  'a Pro owner''s instance is never stamped');

select is(public.remote_claim_lapsed_deletion('00000000-0000-0000-0000-00000000e005'), false,
  'an instance lapsed for 29 days is kept');
select is(public.remote_claim_lapsed_deletion('00000000-0000-0000-0000-00000000e002'), false,
  'an instance that just lapsed is kept');
select is(public.remote_claim_lapsed_deletion('00000000-0000-0000-0000-00000000e004'), false,
  'an instance whose owner subscribed again is kept');
select is(public.remote_claim_lapsed_deletion('00000000-0000-0000-0000-00000000e003'), true,
  'an instance lapsed for 31 days is claimed for deletion');
select is((select status from public.remote_instances where id = '00000000-0000-0000-0000-00000000e003'), 'deleting',
  'the claimed instance moves to deleting');

reset role;
-- A Pro owner's instance with a stale lapse time is never claimed.
update public.remote_instances set lapsed_at = now() - interval '60 days'
  where id = '00000000-0000-0000-0000-00000000e001';
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.remote_claim_lapsed_deletion('00000000-0000-0000-0000-00000000e001'), false,
  'a Pro owner''s instance is never claimed, whatever its lapse time says');
select is(public.remote_sync_instance_lapse('00000000-0000-0000-0000-00000000e001'), null,
  'syncing clears the stale lapse of a Pro owner');

reset role;
update public.remote_instances set status = 'deleted'
  where id = '00000000-0000-0000-0000-00000000e003';
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.remote_claim_lapsed_deletion('00000000-0000-0000-0000-00000000e003'), false,
  'a deleted instance is not claimed again');
select is(public.remote_sync_instance_lapse('00000000-0000-0000-0000-00000000e003'), null,
  'a deleted instance is not stamped');

-- d5 subscribes inside the grace period.
reset role;
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_enf_grace', 'cus_enf_grace', 'user', '00000000-0000-0000-0000-0000000000d5', 'pro', 'active', now());
select is((select lapsed_at from public.remote_instances where id = '00000000-0000-0000-0000-00000000e005'), null,
  'subscribing inside the grace period clears the lapse');
-- A subscription that is not entitled leaves the lapse alone.
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_enf_incomplete', 'cus_enf_incomplete', 'user', '00000000-0000-0000-0000-0000000000d2', 'pro', 'incomplete', now());
select isnt((select lapsed_at from public.remote_instances where id = '00000000-0000-0000-0000-00000000e002'), null,
  'an incomplete subscription does not clear the lapse');

select * from finish();
rollback;
