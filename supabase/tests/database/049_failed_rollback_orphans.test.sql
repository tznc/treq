-- A migration whose rollback ended `failed` (migration-engine.ts,
-- MAX_ROLLBACK_ATTEMPTS) frees the instance and leaves its candidate
-- to the orphan sweep.
begin;
select plan(2);

insert into auth.users (id, email) values ('00000000-0000-0000-0000-00000000f0a1', 'rb@example.com');
insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id, provider_resource_id, fence)
values ('00000000-0000-0000-0000-00000000f1f1', '00000000-0000-0000-0000-00000000f0a1', 'boat', 'us_east', 'small', 'ready', 's', 'boat:small', 'bx_src', 1);
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, provider_resource_id, created_at)
values
  ('00000000-0000-0000-0000-00000000f2a1', '00000000-0000-0000-0000-00000000f1f1', '00000000-0000-0000-0000-00000000f0a1',
   'boat', 'boat:small', 'us_east', 'ready', 'active', 'bx_src', now()),
  ('00000000-0000-0000-0000-00000000f2a2', '00000000-0000-0000-0000-00000000f1f1', '00000000-0000-0000-0000-00000000f0a1',
   'daytona', 'daytona:s', 'us_east', 'bootstrapping', 'candidate', 'dt_tgt', now() - interval '2 hours');
update public.remote_instances set active_placement_id = '00000000-0000-0000-0000-00000000f2a1'
 where id = '00000000-0000-0000-0000-00000000f1f1';
insert into public.sandbox_migrations (id, instance_id, owner_user_id, source_placement_id, target_placement_id, target_machine_type_id, reason, state)
values ('00000000-0000-0000-0000-00000000f3d1', '00000000-0000-0000-0000-00000000f1f1', '00000000-0000-0000-0000-00000000f0a1',
        '00000000-0000-0000-0000-00000000f2a1', '00000000-0000-0000-0000-00000000f2a2', 'daytona:s', 'user_resize', 'rolling_back');

select public.advance_migration('00000000-0000-0000-0000-00000000f3d1', 'rolling_back', 'failed', '{"message":"403"}');

select is((select count(*)::int from public.orphaned_placements(100) where id = '00000000-0000-0000-0000-00000000f2a2'),
  1, 'the candidate of a failed rollback is listed for the orphan sweep');
select lives_ok(
  $$insert into public.sandbox_migrations (instance_id, owner_user_id, source_placement_id, target_machine_type_id, reason, state)
    values ('00000000-0000-0000-0000-00000000f1f1', '00000000-0000-0000-0000-00000000f0a1',
            '00000000-0000-0000-0000-00000000f2a1', 'boat:small', 'user_resize', 'requested')$$,
  'the instance can start a new migration');

select * from finish();
rollback;
