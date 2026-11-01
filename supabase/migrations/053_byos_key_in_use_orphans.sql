-- BYOS chaos QA fix (2026-11-01).

-- 2. A dead placement whose machine was never deleted (a claimed retire or a
-- failed destroy, see orphaned_placements) still needs the key: the orphan
-- sweep deletes that machine with it.
create or replace function public.delete_provider_credential(p_kind text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_user uuid := (select auth.uid());
begin
  if v_user is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  -- Lock first: a concurrent placement insert holds or waits on a key share
  -- lock on this row, so the check below sees its placement.
  perform 1 from public.remote_provider_credentials
    where owner_user_id = v_user and provider_kind = p_kind
    for update;
  if exists (
    select 1 from public.remote_instance_placements p
      join public.remote_provider_credentials c on c.id = p.credential_id
     where c.owner_user_id = v_user and c.provider_kind = p_kind and (p.role <> 'dead'
            or (p.provider_resource_id is not null and p.state <> 'deleted'))
  ) then
    raise exception 'credential_in_use';
  end if;
  delete from public.remote_provider_credentials
   where owner_user_id = v_user and provider_kind = p_kind;
end;
$$;
