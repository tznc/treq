-- Cloud workspaces need a personal Pro subscription. Cloud usage is a
-- metered price on the owner's own Stripe customer (usage-report.ts); a Team
-- member has none, so their usage was built and then dropped as
-- `no_customer`. Pro features other than cloud keep user_has_pro.

create function treq_internal.user_has_cloud(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1
    from public.billing_subscriptions s
    where s.owner_type = 'user'
      and s.owner_id = p_user_id
      and s.plan = 'pro'
      and treq_internal.subscription_entitles(s.status, s.current_period_end)
  );
$$;

revoke all on function treq_internal.user_has_cloud(uuid) from public, anon, authenticated, service_role;

-- Same contract as has_pro: the caller about themselves, or the service role.
create function public.has_cloud(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (
      p_user_id = (select auth.uid())
      or current_setting('role', true) = 'service_role'
    )
    and treq_internal.user_has_cloud(p_user_id),
    false
  );
$$;

revoke all on function public.has_cloud(uuid) from public, anon;
grant execute on function public.has_cloud(uuid) to authenticated, service_role;

-- The cloud gates and the lapse sweep, as before with the cloud rule.
CREATE OR REPLACE FUNCTION treq_internal.user_can_byos(p_user_id uuid)
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  select treq_internal.user_has_cloud(p_user_id);
$function$

;

CREATE OR REPLACE FUNCTION public.remote_lapse_candidates()
 RETURNS TABLE(instance_id uuid, owner_user_id uuid, status text, provider_resource_id text, lapsed_at timestamp with time zone, owner_has_pro boolean)
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  select i.id, i.owner_user_id, i.status, i.provider_resource_id, i.lapsed_at, e.has_pro
  from public.remote_instances i
  cross join lateral (
    select treq_internal.user_has_cloud(i.owner_user_id) as has_pro
  ) e
  where i.status <> 'deleted'
    and (i.lapsed_at is not null or not e.has_pro)
  order by i.lapsed_at nulls first, i.id;
$function$

;

CREATE OR REPLACE FUNCTION public.remote_sync_instance_lapse(p_instance_id uuid)
 RETURNS timestamp with time zone
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_lapsed_at timestamptz;
begin
  update public.remote_instances i
  set lapsed_at = case
        when treq_internal.user_has_cloud(i.owner_user_id) then null
        else coalesce(i.lapsed_at, now())
      end,
      updated_at = now()
  where i.id = p_instance_id
    and i.status <> 'deleted'
  returning i.lapsed_at into v_lapsed_at;
  return v_lapsed_at;
end;
$function$

;

CREATE OR REPLACE FUNCTION public.remote_claim_lapsed_deletion(p_instance_id uuid)
 RETURNS boolean
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
  with claimed as (
    update public.remote_instances i
    set status = 'deleting', updated_at = now()
    where i.id = p_instance_id
      and i.status <> 'deleted'
      and i.lapsed_at is not null
      and i.lapsed_at <= now() - interval '30 days'
      and not treq_internal.user_has_cloud(i.owner_user_id)
    returning 1
  )
  select exists (select 1 from claimed);
$function$

;

CREATE OR REPLACE FUNCTION treq_internal.clear_remote_lapse_on_entitlement()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
begin
  if new.owner_id is null then
    return null;
  end if;
  update public.remote_instances r
  set lapsed_at = null, updated_at = now()
  where r.lapsed_at is not null
    and r.status <> 'deleted'
    and (
      (new.owner_type = 'user' and r.owner_user_id = new.owner_id)
      or (
        new.owner_type = 'organization'
        and r.owner_user_id in (
          select m.user_id from public.organization_members m where m.org_id = new.owner_id
        )
      )
    )
    and treq_internal.user_has_cloud(r.owner_user_id);
  return null;
end;
$function$

;

