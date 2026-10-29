-- Idle controller state (supabase/functions/_shared/remote/idle-controller.ts).
-- remote-admin `enforce_idle` probes each running workspace and stops it
-- after TREQ_IDLE_TIMEOUT_MIN without human or agent activity.

-- One row per instance: the controller's last probe and idle clock.
create table public.remote_instance_activity (
  instance_id uuid primary key references public.remote_instances (id) on delete cascade,
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  placement_id uuid references public.remote_instance_placements (id) on delete set null,
  -- A machine that comes back from 'stopped' restarts its idle clock.
  last_state text not null check (last_state in ('running', 'stopped')),
  probed_at timestamptz not null,
  -- null when the machine was seen stopped and not probed.
  probe_ok boolean,
  probe_failures int not null default 0 check (probe_failures >= 0),
  last_active_at timestamptz,
  idle_timeout_min int not null check (idle_timeout_min > 0),
  stopped_at timestamptz,
  -- activity-probe.ts ProbeSummary of the last successful probe.
  summary jsonb,
  updated_at timestamptz not null default now()
);

-- Processes the user chose to keep running, matched by pid or substring.
create table public.remote_process_pins (
  id uuid primary key default gen_random_uuid(),
  instance_id uuid not null references public.remote_instances (id) on delete cascade,
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  pid int check (pid > 1),
  -- The command seen for `pid` when pinned, so a recycled pid does not match.
  command text,
  pattern text check (char_length(pattern) between 1 and 200),
  expires_at timestamptz not null,
  created_at timestamptz not null default now(),
  check ((pid is null) <> (pattern is null))
);

create index remote_instance_activity_owner_user_id_idx
  on public.remote_instance_activity (owner_user_id);
create index remote_process_pins_owner_user_id_idx
  on public.remote_process_pins (owner_user_id);
create index remote_process_pins_instance on public.remote_process_pins (instance_id, expires_at);

alter table public.remote_instance_activity enable row level security;
alter table public.remote_process_pins enable row level security;
-- Owners read through get_workspace_activity() and write through the
-- remote-instance Edge Function.
revoke all on public.remote_instance_activity, public.remote_process_pins from anon, authenticated;

-- Active placements to look at: running ones to probe, and suspended ones a
-- provider may have started again. migration_state lets the controller
-- exempt only a migration that builds or cuts over to a target.
create function public.remote_idle_candidates()
returns table (
  instance_id uuid,
  owner_user_id uuid,
  placement_id uuid,
  provider_kind text,
  provider_resource_id text,
  state text,
  last_state text,
  last_active_at timestamptz,
  probe_failures int,
  migration_open boolean,
  migration_state text
)
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return query
  select i.id, i.owner_user_id, p.id, p.provider_kind, p.provider_resource_id, p.state,
         a.last_state, a.last_active_at, coalesce(a.probe_failures, 0),
         m.state is not null, m.state
    from public.remote_instances i
    join public.remote_instance_placements p on p.id = i.active_placement_id
    left join public.remote_instance_activity a on a.instance_id = i.id
    left join public.sandbox_migrations m
      on m.instance_id = i.id
     and m.state not in ('completed', 'rolled_back', 'failed', 'cancelled')
   where p.role = 'active'
     and p.provider_resource_id is not null
     and p.state in ('ready', 'waking', 'degraded', 'suspended')
   order by i.id;
end;
$$;

-- The caller's workspace: the last probe's processes, live pins,
-- last_active_at and when it stops if it stays idle. null with no workspace.
create function public.get_workspace_activity()
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
  select jsonb_build_object(
    'instance_id', i.id,
    'status', i.status,
    'probed_at', a.probed_at,
    'probe_ok', a.probe_ok,
    'last_active_at', a.last_active_at,
    'idle_timeout_min', a.idle_timeout_min,
    'stops_at', case
      when i.status in ('ready', 'waking', 'degraded') and a.last_active_at is not null
        then a.last_active_at + make_interval(mins => a.idle_timeout_min)
    end,
    'active', coalesce((a.summary ->> 'active')::boolean, false),
    'reasons', coalesce(a.summary -> 'reasons', '[]'::jsonb),
    'processes', coalesce(a.summary -> 'processes', '[]'::jsonb),
    'pins', coalesce((
      select jsonb_agg(jsonb_build_object(
               'id', pin.id, 'pid', pin.pid, 'command', pin.command,
               'pattern', pin.pattern, 'expires_at', pin.expires_at) order by pin.created_at)
        from public.remote_process_pins pin
       where pin.instance_id = i.id and pin.expires_at > now()
    ), '[]'::jsonb)
  )
    from public.remote_instances i
    left join public.remote_instance_activity a on a.instance_id = i.id
   where i.owner_user_id = (select auth.uid())
     and i.status <> 'deleted'
   order by i.created_at desc
   limit 1;
$$;

revoke all on function public.remote_idle_candidates() from public, anon, authenticated;
grant execute on function public.remote_idle_candidates() to service_role;
revoke all on function public.get_workspace_activity() from public, anon;
grant execute on function public.get_workspace_activity() to authenticated, service_role;
