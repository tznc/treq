-- Placement lifecycle under concurrency (038, 039, 041): one candidate
-- per instance, leased state changes, cancels that cannot miss,
-- the fence captured at creation, atomic reverse cutover, orphan listing,
-- spend-cap rollbacks and the idle exemption's migration state.
begin;
select plan(33);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000048e1', 'life-owner@example.com');

-- Instance 1: active src (a1), retiring old (a3) and a candidate (a2).
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id, provider_resource_id, fence)
values ('00000000-0000-0000-0000-0000000048f1', '00000000-0000-0000-0000-0000000048e1',
        'boat', 'us_east', 'small', 'ready', 's', 'boat:small', 'bx_src', 4);
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, provider_resource_id)
values
  ('00000000-0000-0000-0000-0000000048a1', '00000000-0000-0000-0000-0000000048f1',
   '00000000-0000-0000-0000-0000000048e1', 'boat', 'boat:small', 'us_east', 'ready', 'active', 'bx_src'),
  ('00000000-0000-0000-0000-0000000048a2', '00000000-0000-0000-0000-0000000048f1',
   '00000000-0000-0000-0000-0000000048e1', 'daytona', 'daytona:s', 'us_east', 'bootstrapping', 'candidate', 'dt_tgt');
update public.remote_instances set active_placement_id = '00000000-0000-0000-0000-0000000048a1'
 where id = '00000000-0000-0000-0000-0000000048f1';

-- ── One candidate per instance ──────────────────────────────────────────

select throws_ok(
  $$insert into public.remote_instance_placements (instance_id, owner_user_id, provider_kind, machine_type_id, region, role)
    values ('00000000-0000-0000-0000-0000000048f1', '00000000-0000-0000-0000-0000000048e1',
            'daytona', 'daytona:s', 'us_east', 'candidate')$$,
  '23505', null, 'an instance has at most one candidate placement');

-- ── The fence is captured at creation ───────────────────────────────────

insert into public.sandbox_migrations (id, instance_id, owner_user_id, source_placement_id, target_placement_id, target_machine_type_id, reason, state)
values ('00000000-0000-0000-0000-0000000048d1', '00000000-0000-0000-0000-0000000048f1',
        '00000000-0000-0000-0000-0000000048e1', '00000000-0000-0000-0000-0000000048a1',
        '00000000-0000-0000-0000-0000000048a2', 'daytona:s', 'user_resize', 'target_provisioning');
select is((select expected_fence from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000048d1'),
  4::bigint, 'a new migration records the instance fence');

-- ── Leases ──────────────────────────────────────────────────────────────

select is((select count(*)::int from public.claim_sandbox_migrations('worker-a', 120, 5, '00000000-0000-0000-0000-0000000048d1')),
  1, 'worker-a claims the migration');
select is(public.renew_sandbox_migration_lease('00000000-0000-0000-0000-0000000048d1', 'worker-a', 1200), true,
  'the holder extends its lease');
select ok((select lease_expires_at >= now() + interval '1199 seconds' from public.sandbox_migrations
            where id = '00000000-0000-0000-0000-0000000048d1'),
  'the lease now covers the long step');
select is(public.renew_sandbox_migration_lease('00000000-0000-0000-0000-0000000048d1', 'worker-b', 1200), false,
  'another worker cannot extend it');
select is(public.advance_migration_leased('00000000-0000-0000-0000-0000000048d1', 'target_provisioning',
  'target_bootstrapped', null, 'worker-b'), false, 'a worker without the lease cannot advance');
select is((select state from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000048d1'),
  'target_provisioning', 'the state is unchanged');
select is(public.advance_migration_leased('00000000-0000-0000-0000-0000000048d1', 'target_provisioning',
  'cutover', null, 'worker-a'), true, 'the lease holder advances');

-- ── Cutover requires the creation fence and the source still active ─────

update public.remote_instances set fence = 5 where id = '00000000-0000-0000-0000-0000000048f1';
select throws_ok(
  $$select public.cutover_migration('00000000-0000-0000-0000-0000000048d1', '00000000-0000-0000-0000-0000000048a2')$$,
  'P0001', 'fence_mismatch', 'a fence moved since creation refuses cutover');
update public.remote_instances set fence = 4 where id = '00000000-0000-0000-0000-0000000048f1';

update public.remote_instance_placements set role = 'retiring' where id = '00000000-0000-0000-0000-0000000048a1';
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, provider_resource_id)
values ('00000000-0000-0000-0000-0000000048a9', '00000000-0000-0000-0000-0000000048f1',
        '00000000-0000-0000-0000-0000000048e1', 'boat', 'boat:small', 'us_east', 'ready', 'active', 'bx_other');
update public.remote_instances set active_placement_id = '00000000-0000-0000-0000-0000000048a9'
 where id = '00000000-0000-0000-0000-0000000048f1';
select throws_ok(
  $$select public.cutover_migration('00000000-0000-0000-0000-0000000048d1', '00000000-0000-0000-0000-0000000048a2')$$,
  'P0001', 'source_not_active', 'cutover refuses once the source is no longer active');
-- Put the source back.
update public.remote_instance_placements set role = 'dead', state = 'deleted', provider_resource_id = null
 where id = '00000000-0000-0000-0000-0000000048a9';
update public.remote_instance_placements set role = 'active' where id = '00000000-0000-0000-0000-0000000048a1';
update public.remote_instances set active_placement_id = '00000000-0000-0000-0000-0000000048a1'
 where id = '00000000-0000-0000-0000-0000000048f1';

select is(public.cutover_migration('00000000-0000-0000-0000-0000000048d1', '00000000-0000-0000-0000-0000000048a2'),
  5::bigint, 'cutover with the creation fence returns the new fence');
select is((select role from public.remote_instance_placements where id = '00000000-0000-0000-0000-0000000048a1'),
  'retiring', 'the source retires');
select is(public.advance_migration('00000000-0000-0000-0000-0000000048d1', 'cutover', 'completed'), true,
  'the migration completes');

-- ── Reverse cutover ─────────────────────────────────────────────────────

-- Another open migration blocks it.
insert into public.sandbox_migrations (id, instance_id, owner_user_id, source_placement_id, target_machine_type_id, reason, state)
values ('00000000-0000-0000-0000-0000000048d2', '00000000-0000-0000-0000-0000000048f1',
        '00000000-0000-0000-0000-0000000048e1', '00000000-0000-0000-0000-0000000048a2', 'boat:small', 'drain', 'requested');
select throws_ok(
  $$select public.reverse_cutover('00000000-0000-0000-0000-0000000048f1', '00000000-0000-0000-0000-0000000048d1')$$,
  'P0001', 'migration_open', 'reverse cutover is refused while another migration is open');

-- ── cancel_open_migration ───────────────────────────────────────────────

select is(public.cancel_open_migration('00000000-0000-0000-0000-0000000048f1', '{"reason":"instance_deleted"}'),
  '00000000-0000-0000-0000-0000000048d2'::uuid, 'the open migration is cancelled whatever its state');
select is((select state from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000048d2'),
  'cancelled', 'it is cancelled');
select is((select count(*)::int from public.sandbox_migration_steps
            where migration_id = '00000000-0000-0000-0000-0000000048d2' and to_state = 'cancelled'),
  1, 'the cancel is logged as a step');
select is(public.cancel_open_migration('00000000-0000-0000-0000-0000000048f1', null), null::uuid,
  'nothing open: nothing cancelled');

-- A source that is no longer retiring cannot come back.
update public.remote_instance_placements set role = 'dead' where id = '00000000-0000-0000-0000-0000000048a1';
select throws_ok(
  $$select public.reverse_cutover('00000000-0000-0000-0000-0000000048f1', '00000000-0000-0000-0000-0000000048d1')$$,
  'P0001', 'source_not_retiring', 'a destroyed source cannot be reversed to');
update public.remote_instance_placements set role = 'retiring' where id = '00000000-0000-0000-0000-0000000048a1';

select is(public.reverse_cutover('00000000-0000-0000-0000-0000000048f1', '00000000-0000-0000-0000-0000000048d1',
  '{"reversed":true}'), 6::bigint, 'reverse cutover bumps the fence');
select is((select active_placement_id from public.remote_instances where id = '00000000-0000-0000-0000-0000000048f1'),
  '00000000-0000-0000-0000-0000000048a1'::uuid, 'the source is active again');
select is((select array[role, state] from public.remote_instance_placements where id = '00000000-0000-0000-0000-0000000048a1'),
  array['active', 'ready'], 'the source is active and ready');
select is((select role from public.remote_instance_placements where id = '00000000-0000-0000-0000-0000000048a2'),
  'retiring', 'the switched-to machine retires');
select is((select state from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000048d1'),
  'rolled_back', 'the migration is rolled back in the same transaction');

-- ── Orphans ─────────────────────────────────────────────────────────────

-- A dead placement whose machine was never deleted, a deleted one, and an
-- abandoned candidate with no open migration.
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, provider_resource_id, created_at)
values
  ('00000000-0000-0000-0000-0000000048b1', '00000000-0000-0000-0000-0000000048f1',
   '00000000-0000-0000-0000-0000000048e1', 'boat', 'boat:small', 'us_east', 'suspended', 'dead', 'bx_orphan', now() - interval '2 days'),
  ('00000000-0000-0000-0000-0000000048b3', '00000000-0000-0000-0000-0000000048f1',
   '00000000-0000-0000-0000-0000000048e1', 'boat', 'boat:small', 'us_east', 'provisioning', 'candidate', 'bx_cand', now() - interval '2 hours');
select results_eq(
  $$select id from public.orphaned_placements(10) order by id$$,
  $$values ('00000000-0000-0000-0000-0000000048b1'::uuid), ('00000000-0000-0000-0000-0000000048b3'::uuid)$$,
  'dead placements still holding a machine and abandoned candidates are orphans');
insert into public.sandbox_migrations (id, instance_id, owner_user_id, source_placement_id, target_machine_type_id, reason, state)
values ('00000000-0000-0000-0000-0000000048d3', '00000000-0000-0000-0000-0000000048f1',
        '00000000-0000-0000-0000-0000000048e1', '00000000-0000-0000-0000-0000000048a1', 'boat:small', 'user_resize', 'target_provisioning');
select results_eq(
  $$select id from public.orphaned_placements(10)$$,
  $$values ('00000000-0000-0000-0000-0000000048b1'::uuid)$$,
  'a candidate an open migration owns is not an orphan');

-- ── Idle exemption carries the migration state ──────────────────────────

insert into public.remote_instance_activity
  (instance_id, owner_user_id, placement_id, last_state, probed_at, probe_ok, probe_failures, last_active_at, idle_timeout_min)
values ('00000000-0000-0000-0000-0000000048f1', '00000000-0000-0000-0000-0000000048e1',
        '00000000-0000-0000-0000-0000000048a1', 'running', now(), true, 0, now(), 15);
select is((select migration_state from public.remote_idle_candidates()
            where instance_id = '00000000-0000-0000-0000-0000000048f1'),
  'target_provisioning', 'the idle listing names the open migration''s state');

-- ── Spend cap rolls back open resizes ───────────────────────────────────

select results_eq(
  $$select migration_id from public.rollback_user_resizes('00000000-0000-0000-0000-0000000048e1')$$,
  $$values ('00000000-0000-0000-0000-0000000048d3'::uuid)$$,
  'an open resize that has not cut over is sent to rollback');
select is((select state from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000048d3'),
  'rolling_back', 'the engine will delete its candidate');
update public.sandbox_migrations set state = 'source_retiring' where id = '00000000-0000-0000-0000-0000000048d3';
select is((select count(*)::int from public.rollback_user_resizes('00000000-0000-0000-0000-0000000048e1')),
  0, 'a resize past cutover is left alone');

-- ── Service role only ───────────────────────────────────────────────────

select ok(not has_function_privilege('authenticated', 'public.reverse_cutover(uuid, uuid, jsonb)', 'execute'),
  'signed-in users cannot reverse a cutover');
select ok(not has_function_privilege('authenticated', 'public.cancel_open_migration(uuid, jsonb)', 'execute'),
  'signed-in users cannot cancel migrations');

select * from finish();
rollback;
