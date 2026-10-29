-- Synced app and repo configs: small files stored inline, versioned per
-- scope with optimistic concurrency.

create table public.user_config_snapshots (
  id uuid primary key default gen_random_uuid(),
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  scope text not null check (scope = 'app' or scope like 'repo:%'),
  version bigint not null check (version > 0),
  files jsonb not null,                  -- {relative path: base64 content}
  created_at timestamptz not null default now(),
  unique (owner_user_id, scope, version),
  check (octet_length(files::text) <= 1048576)
);

alter table public.user_config_snapshots enable row level security;
revoke all on public.user_config_snapshots from anon, authenticated;

create function public.put_user_config(p_scope text, p_expected_version bigint, p_files jsonb)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_owner uuid := (select auth.uid());
  v_latest bigint;
begin
  if v_owner is null then
    raise exception 'not signed in' using errcode = '42501';
  end if;
  perform pg_advisory_xact_lock(hashtext(v_owner::text || p_scope));
  select coalesce(max(version), 0) into v_latest
    from public.user_config_snapshots
   where owner_user_id = v_owner and scope = p_scope;
  if v_latest <> p_expected_version then
    raise exception 'version_conflict';
  end if;
  insert into public.user_config_snapshots (owner_user_id, scope, version, files)
  values (v_owner, p_scope, v_latest + 1, p_files);
  -- Keep the last 20 versions per scope.
  delete from public.user_config_snapshots
   where owner_user_id = v_owner and scope = p_scope and version <= v_latest + 1 - 20;
  return v_latest + 1;
end;
$$;

create function public.get_user_config(p_scope text)
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
  select jsonb_build_object('version', s.version, 'files', s.files, 'created_at', s.created_at)
    from public.user_config_snapshots s
   where s.owner_user_id = (select auth.uid()) and s.scope = p_scope
   order by s.version desc
   limit 1;
$$;

create function public.list_user_config_scopes()
returns table (scope text, version bigint)
language sql
stable
security definer
set search_path = ''
as $$
  select s.scope, max(s.version)
    from public.user_config_snapshots s
   where s.owner_user_id = (select auth.uid())
   group by s.scope;
$$;

revoke all on function public.put_user_config(text, bigint, jsonb) from public, anon;
revoke all on function public.get_user_config(text) from public, anon;
revoke all on function public.list_user_config_scopes() from public, anon;
grant execute on function public.put_user_config(text, bigint, jsonb) to authenticated;
grant execute on function public.get_user_config(text) to authenticated;
grant execute on function public.list_user_config_scopes() to authenticated;
