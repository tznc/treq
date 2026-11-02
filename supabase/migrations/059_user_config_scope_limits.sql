-- Bounds synced config storage per user. Each snapshot was capped at 1 MiB
-- and 20 versions per scope, but nothing limited the number of scopes, so
-- any signed-in user could grow storage without bound: at most 100 scopes,
-- each name at most 256 characters.

create or replace function public.put_user_config(p_scope text, p_expected_version bigint, p_files jsonb)
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
  if char_length(p_scope) > 256 then
    raise exception 'config_scope_too_long';
  end if;
  -- Serialize per owner so two new scopes cannot both pass the count.
  perform pg_advisory_xact_lock(hashtext('treq-config-scopes:' || v_owner::text));
  perform pg_advisory_xact_lock(hashtext(v_owner::text || p_scope));
  select coalesce(max(version), 0) into v_latest
    from public.user_config_snapshots
   where owner_user_id = v_owner and scope = p_scope;
  if v_latest <> p_expected_version then
    raise exception 'version_conflict';
  end if;
  if v_latest = 0 and (
    select count(distinct s.scope) from public.user_config_snapshots s where s.owner_user_id = v_owner
  ) >= 100 then
    raise exception 'config_scope_limit';
  end if;
  insert into public.user_config_snapshots (owner_user_id, scope, version, files)
  values (v_owner, p_scope, v_latest + 1, p_files);
  -- Keep the last 20 versions per scope.
  delete from public.user_config_snapshots
   where owner_user_id = v_owner and scope = p_scope and version <= v_latest + 1 - 20;
  return v_latest + 1;
end;
$$;
