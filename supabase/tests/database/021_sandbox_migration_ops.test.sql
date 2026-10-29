-- Migration leases, repo remote URLs and operator price changes (038, 039).
begin;
select plan(14);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000e1', 'mig-ops@example.com');

select has_column('public', 'remote_repositories', 'remote_url', 'remote_repositories has remote_url');
select col_is_null('public', 'remote_audit_events', 'owner_user_id', 'operator audit events may have no owner');

insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status, tier_id, machine_type_id)
values ('00000000-0000-0000-0000-0000000000f1', '00000000-0000-0000-0000-0000000000e1',
        'local', 'us_east', 'small', 'ready', 's', 'local:s');
insert into public.remote_instance_placements (id, instance_id, owner_user_id, provider_kind, machine_type_id, region, state, role)
values ('00000000-0000-0000-0000-0000000000a1', '00000000-0000-0000-0000-0000000000f1',
        '00000000-0000-0000-0000-0000000000e1', 'local', 'local:s', 'us_east', 'ready', 'active');
insert into public.sandbox_migrations (id, instance_id, owner_user_id, source_placement_id, target_machine_type_id, reason, state)
values ('00000000-0000-0000-0000-0000000000b1', '00000000-0000-0000-0000-0000000000f1',
        '00000000-0000-0000-0000-0000000000e1', '00000000-0000-0000-0000-0000000000a1', 'boat:small', 'drain', 'requested');

insert into public.remote_instance_operations (owner_user_id, instance_id, operation_type, idempotency_key, status)
values ('00000000-0000-0000-0000-0000000000e1', '00000000-0000-0000-0000-0000000000f1', 'resize', 'k1', 'succeeded');
select ok(true, 'resize is an allowed operation type');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(
  (select array_agg(migration_id) from public.claim_sandbox_migrations('worker-a', 60, 10)),
  array['00000000-0000-0000-0000-0000000000b1'::uuid],
  'a worker claims a runnable migration');
select is(
  (select count(*)::int from public.claim_sandbox_migrations('worker-b', 60, 10)),
  0,
  'a second worker cannot claim a leased migration');
select is(
  (select count(*)::int from public.claim_sandbox_migrations('worker-a', 60, 10, '00000000-0000-0000-0000-0000000000b1')),
  1,
  'the lease holder can renew its lease');
select public.release_sandbox_migration('00000000-0000-0000-0000-0000000000b1', 'worker-a');
select is(
  (select count(*)::int from public.claim_sandbox_migrations('worker-b', 60, 10, '00000000-0000-0000-0000-0000000000b1')),
  1,
  'a released migration can be claimed by another worker');

reset role;
update public.sandbox_migrations set lease_expires_at = now() - interval '1 second';
update public.sandbox_migrations set state = 'awaiting_ack';
set local role service_role;
select is(
  (select count(*)::int from public.claim_sandbox_migrations('worker-c', 60, 10)),
  0,
  'a migration waiting for the user is not claimed');
reset role;
update public.sandbox_migrations set state = 'cutover';
set local role service_role;
select is(
  (select count(*)::int from public.claim_sandbox_migrations('worker-c', 60, 10)),
  1,
  'an expired lease can be taken over');

-- Price changes close the current row and open a new one.
select isnt(
  public.set_machine_type_price('boat:small', 999999, 0, now() + interval '1 minute'),
  null,
  'set_machine_type_price returns the new price id');
reset role;
select is(
  (select count(*)::int from public.machine_type_prices where machine_type_id = 'boat:small' and effective_to is null),
  1,
  'exactly one open price row remains');
select is(
  (select running_nanousd_per_sec from public.machine_type_prices
    where machine_type_id = 'boat:small' and effective_to is null),
  999999::bigint,
  'the new price is the open one');

set local role service_role;
select throws_ok(
  $$select public.set_machine_type_price('boat:small', 1, 0, now() - interval '10 years')$$,
  'effective_from must be after the current price',
  'a new price cannot start before the current one');
reset role;

set local role authenticated;
select throws_ok(
  $$select public.claim_sandbox_migrations('x', 60, 1)$$,
  '42501',
  null,
  'customers cannot claim migrations');
reset role;

select * from finish();
rollback;
