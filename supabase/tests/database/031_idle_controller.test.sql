-- Idle controller state, process pins and the owner's activity view
-- (049_idle_controller.sql).
begin;
select plan(16);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000043e1', 'idle-owner@example.com'),
  ('00000000-0000-0000-0000-0000000043e2', 'idle-other@example.com');

insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id, provider_resource_id)
values ('00000000-0000-0000-0000-0000000043f1', '00000000-0000-0000-0000-0000000043e1',
        'boat', 'us_east', 'small', 'ready', 's', 'boat:small', 'bx_1');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role, provider_resource_id)
values ('00000000-0000-0000-0000-0000000043a1', '00000000-0000-0000-0000-0000000043f1',
        '00000000-0000-0000-0000-0000000043e1', 'boat', 'boat:small', 'us_east', 'ready', 'active', 'bx_1');
update public.remote_instances set active_placement_id = '00000000-0000-0000-0000-0000000043a1'
 where id = '00000000-0000-0000-0000-0000000043f1';

select ok((select relrowsecurity from pg_class where oid = 'public.remote_instance_activity'::regclass),
  'remote_instance_activity has RLS');
select ok((select relrowsecurity from pg_class where oid = 'public.remote_process_pins'::regclass),
  'remote_process_pins has RLS');

select throws_ok(
  $$insert into public.remote_process_pins (instance_id, owner_user_id, pid, pattern, expires_at)
    values ('00000000-0000-0000-0000-0000000043f1', '00000000-0000-0000-0000-0000000043e1', 5, 'x', now())$$,
  '23514', null, 'a pin names a pid or a pattern, not both');

-- The controller's listing (service role only).
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select results_eq(
  $$select instance_id, placement_id, provider_kind, provider_resource_id, state, last_state, migration_open
      from public.remote_idle_candidates()$$,
  $$values ('00000000-0000-0000-0000-0000000043f1'::uuid, '00000000-0000-0000-0000-0000000043a1'::uuid,
            'boat'::text, 'bx_1'::text, 'ready'::text, null::text, false)$$,
  'an active running placement is an idle candidate');

insert into public.remote_instance_activity
  (instance_id, owner_user_id, placement_id, last_state, probed_at, probe_ok, probe_failures,
   last_active_at, idle_timeout_min, summary)
values ('00000000-0000-0000-0000-0000000043f1', '00000000-0000-0000-0000-0000000043e1',
        '00000000-0000-0000-0000-0000000043a1', 'running', now(), true, 0,
        now() - interval '5 minutes', 15,
        '{"active":false,"reasons":[],"ssh_sessions":0,"agent_turns":0,"pty_sessions":0,"idle_sec":null,"live_pins":[],
          "processes":[{"pid":500,"command":"node server.js","origin":"orphan","cpu":12.5,"ports":[3000],"pinned":false}]}');
insert into public.remote_process_pins (id, instance_id, owner_user_id, pattern, expires_at) values
  ('00000000-0000-0000-0000-0000000043c1', '00000000-0000-0000-0000-0000000043f1',
   '00000000-0000-0000-0000-0000000043e1', 'server.js', now() + interval '1 hour'),
  ('00000000-0000-0000-0000-0000000043c2', '00000000-0000-0000-0000-0000000043f1',
   '00000000-0000-0000-0000-0000000043e1', 'old', now() - interval '1 hour');

select is((select last_state from public.remote_idle_candidates()), 'running',
  'the candidate carries the last state the controller saw');
reset role;

insert into public.sandbox_migrations (instance_id, owner_user_id, source_placement_id, target_machine_type_id, reason, state)
values ('00000000-0000-0000-0000-0000000043f1', '00000000-0000-0000-0000-0000000043e1',
        '00000000-0000-0000-0000-0000000043a1', 'daytona:s', 'drain', 'requested');
set local role service_role;
select is((select migration_open from public.remote_idle_candidates()), true,
  'an open migration is flagged');
reset role;
update public.sandbox_migrations set state = 'cancelled';

-- The owner.
set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000043e1","role":"authenticated"}', true);

select throws_ok($$select * from public.remote_idle_candidates()$$, '42501', null,
  'users cannot list idle candidates');
select throws_ok($$select * from public.remote_instance_activity$$, '42501', null,
  'users cannot read the activity table directly');
select throws_ok($$select * from public.remote_process_pins$$, '42501', null,
  'users cannot read pins directly');

select is((public.get_workspace_activity() ->> 'instance_id')::uuid, '00000000-0000-0000-0000-0000000043f1'::uuid,
  'the owner sees their workspace activity');
select is((public.get_workspace_activity() -> 'processes' -> 0 ->> 'command'), 'node server.js',
  'processes come from the last probe');
select is((public.get_workspace_activity() ->> 'idle_timeout_min')::int, 15, 'the idle timeout is returned');
select ok(
  abs(extract(epoch from ((public.get_workspace_activity() ->> 'stops_at')::timestamptz - (now() + interval '10 minutes')))) < 1,
  'stops_at is last_active_at plus the idle timeout');
select is(jsonb_array_length(public.get_workspace_activity() -> 'pins'), 1, 'only unexpired pins are listed');
select is(public.get_workspace_activity() -> 'pins' -> 0 ->> 'pattern', 'server.js', 'the pin is returned');

-- Someone else.
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000043e2","role":"authenticated"}', true);
select is(public.get_workspace_activity(), null, 'another user sees nothing');

select * from finish();
rollback;
