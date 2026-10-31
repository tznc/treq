-- A lapsed cloud workspace is now kept 30 days, not 7, before the lapse
-- sweep deletes it. Resubscribing within that window restores it.

comment on column public.remote_instances.lapsed_at is
  'When the owner''s Pro entitlement was found to have ended. Cleared when Pro returns. The lapse sweep deletes the instance 30 days after this.';

-- Moves an instance to 'deleting' when its owner has had no Pro for 30 days.
-- Returns whether it did. The sweep deletes the provider resource only after
-- this returns true.
create or replace function public.remote_claim_lapsed_deletion(p_instance_id uuid)
returns boolean
language sql
security definer
set search_path = ''
as $$
  with claimed as (
    update public.remote_instances i
    set status = 'deleting', updated_at = now()
    where i.id = p_instance_id
      and i.status <> 'deleted'
      and i.lapsed_at is not null
      and i.lapsed_at <= now() - interval '30 days'
      and not treq_internal.user_has_pro(i.owner_user_id)
    returning 1
  )
  select exists (select 1 from claimed);
$$;
