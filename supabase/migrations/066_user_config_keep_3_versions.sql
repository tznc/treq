-- Synced config keeps the last 3 versions of each scope (was 20). A trigger
-- prunes after each save, so this does not depend on which put_user_config
-- definition is current, and existing history is trimmed once below.

create function treq_internal.prune_user_config_versions()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  delete from public.user_config_snapshots
   where owner_user_id = new.owner_user_id
     and scope = new.scope
     and version <= new.version - 3;
  return null;
end;
$$;

revoke all on function treq_internal.prune_user_config_versions() from public, anon, authenticated;

create trigger user_config_snapshots_keep_3_versions
  after insert on public.user_config_snapshots
  for each row execute function treq_internal.prune_user_config_versions();

delete from public.user_config_snapshots s
 using (
   select owner_user_id, scope, max(version) as latest
     from public.user_config_snapshots
    group by owner_user_id, scope
 ) l
 where s.owner_user_id = l.owner_user_id
   and s.scope = l.scope
   and s.version <= l.latest - 3;
