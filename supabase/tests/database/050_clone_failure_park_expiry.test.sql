-- Migrations parked on a failed re-clone expire after their deadline
-- (070): the hourly run moves owner resizes past the deadline to
-- rolling_back, which deletes the stopped new machine. Preflight parks,
-- unexpired parks and operator drains are left alone.
begin;
select plan(8);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000050e1', 'park-owner-1@example.com'),
  ('00000000-0000-0000-0000-0000000050e2', 'park-owner-2@example.com'),
  ('00000000-0000-0000-0000-0000000050e3', 'park-owner-3@example.com'),
  ('00000000-0000-0000-0000-0000000050e4', 'park-owner-4@example.com');

-- Four instances, each with an active source (aN1) and a candidate (aN2).
do $$
declare n int;
begin
  for n in 1..4 loop
    insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id, provider_resource_id, fence)
    values (('00000000-0000-0000-0000-00000000500' || n)::uuid, ('00000000-0000-0000-0000-0000000050e' || n)::uuid,
            'boat', 'us_east', 'small', 'ready', 's', 'boat:small', 'bx_src' || n, 0);
    insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, provider_resource_id)
    values
      (('00000000-0000-0000-0000-0000000050' || n || '1')::uuid, ('00000000-0000-0000-0000-00000000500' || n)::uuid,
       ('00000000-0000-0000-0000-0000000050e' || n)::uuid, 'boat', 'boat:small', 'us_east', 'ready', 'active', 'bx_src' || n),
      (('00000000-0000-0000-0000-0000000050' || n || '2')::uuid, ('00000000-0000-0000-0000-00000000500' || n)::uuid,
       ('00000000-0000-0000-0000-0000000050e' || n)::uuid, 'boat', 'boat:large', 'us_east', 'suspended', 'candidate', 'bx_tgt' || n);
    update public.remote_instances set active_placement_id = ('00000000-0000-0000-0000-0000000050' || n || '1')::uuid
     where id = ('00000000-0000-0000-0000-00000000500' || n)::uuid;
  end loop;
end $$;

insert into public.sandbox_migrations (id, instance_id, owner_user_id, source_placement_id, target_placement_id, target_machine_type_id, reason, state, deadline)
values
  -- 1: a resize parked on a clone failure, past its deadline.
  ('00000000-0000-0000-0000-0000000050d1', '00000000-0000-0000-0000-000000005001', '00000000-0000-0000-0000-0000000050e1',
   '00000000-0000-0000-0000-000000005011', '00000000-0000-0000-0000-000000005012', 'boat:large', 'user_resize', 'awaiting_ack', now() - interval '1 minute'),
  -- 2: the same, still inside its deadline.
  ('00000000-0000-0000-0000-0000000050d2', '00000000-0000-0000-0000-000000005002', '00000000-0000-0000-0000-0000000050e2',
   '00000000-0000-0000-0000-000000005021', '00000000-0000-0000-0000-000000005022', 'boat:large', 'user_resize', 'awaiting_ack', now() + interval '1 hour'),
  -- 3: a preflight park (no new machine yet), past a deadline.
  ('00000000-0000-0000-0000-0000000050d3', '00000000-0000-0000-0000-000000005003', '00000000-0000-0000-0000-0000000050e3',
   '00000000-0000-0000-0000-000000005031', null, 'boat:large', 'user_resize', 'awaiting_ack', now() - interval '1 minute'),
  -- 4: an operator drain parked on a clone failure, past its deadline.
  ('00000000-0000-0000-0000-0000000050d4', '00000000-0000-0000-0000-000000005004', '00000000-0000-0000-0000-0000000050e4',
   '00000000-0000-0000-0000-000000005041', '00000000-0000-0000-0000-000000005042', 'boat:large', 'drain', 'awaiting_ack', now() - interval '1 minute');

select throws_ok(
  $$set local role authenticated; select public.expire_clone_failure_parks(100)$$,
  '42501', null, 'signed-in users cannot expire parks');
reset role;

select is(
  (select array_agg(id order by id) from public.expire_clone_failure_parks(100)),
  array['00000000-0000-0000-0000-0000000050d1'::uuid],
  'only the expired clone-failure park of a resize is expired');

select is((select state from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000050d1'),
  'rolling_back', 'the expired park moves to rolling_back, so the engine deletes its machine');
select is((select error->>'reason' from public.sandbox_migrations where id = '00000000-0000-0000-0000-0000000050d1'),
  'park_expired', 'the expiry is recorded as the error');
select is((select count(*)::int from public.sandbox_migration_steps
            where migration_id = '00000000-0000-0000-0000-0000000050d1'
              and from_state = 'awaiting_ack' and to_state = 'rolling_back'),
  1, 'the expiry is recorded as a step');
select is((select array_agg(state order by id) from public.sandbox_migrations
            where id in ('00000000-0000-0000-0000-0000000050d2', '00000000-0000-0000-0000-0000000050d3',
                         '00000000-0000-0000-0000-0000000050d4')),
  array['awaiting_ack', 'awaiting_ack', 'awaiting_ack'],
  'unexpired parks, preflight parks and drains stay parked');
select is((select count(*)::int from public.expire_clone_failure_parks(100)), 0,
  'a second run expires nothing');
select ok((select bool_and(m.state = 'rolling_back') from public.claim_sandbox_migrations('w', 60, 10, '00000000-0000-0000-0000-0000000050d1') c
             join public.sandbox_migrations m on m.id = c.migration_id),
  'the hourly run then claims the rolled-back migration');

select * from finish();
rollback;
