-- A migration parked because a repo failed to re-clone (target placement
-- already created) waits for its owner with the new machine stopped. Past
-- its deadline, the hourly run rolls it back so the engine deletes that
-- machine and the instance can start a new migration. Preflight parks
-- (no target yet) and operator drains/retires keep their own handling.
create function public.expire_clone_failure_parks(p_limit int default 100)
returns table (id uuid)
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return query
  with expired as (
    select m.id
      from public.sandbox_migrations m
     where m.state = 'awaiting_ack'
       and m.target_placement_id is not null
       and m.reason not in ('drain', 'retire')
       and m.deadline < now()
     order by m.deadline
     limit greatest(p_limit, 0)
     for update skip locked
  ), moved as (
    update public.sandbox_migrations m
       set state = 'rolling_back',
           error = jsonb_build_object('reason', 'park_expired', 'deadline', m.deadline),
           updated_at = now()
      from expired
     where m.id = expired.id and m.state = 'awaiting_ack'
    returning m.id, m.deadline
  ), logged as (
    insert into public.sandbox_migration_steps (migration_id, from_state, to_state, detail)
    select moved.id, 'awaiting_ack', 'rolling_back', jsonb_build_object('reason', 'park_expired', 'deadline', moved.deadline)
      from moved
    returning migration_id
  )
  select logged.migration_id from logged;
end;
$$;

revoke all on function public.expire_clone_failure_parks(int) from public, anon, authenticated;
grant execute on function public.expire_clone_failure_parks(int) to service_role;
