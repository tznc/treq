-- Team cloud. An organization owner chooses what members may run on the
-- organization's account: treq-managed sandboxes, BYOS on the organization's
-- own provider keys, or both. The owner also sets an organization-wide and a
-- per-member spend cap. A member picks the profile (personal, or an
-- organization) their cloud workspace bills to, in Settings. Usage on an
-- organization profile is billed to the organization's Stripe customer.
--
-- Personal cloud is unchanged: it needs personal Pro, and a personal BYOS key
-- needs personal Pro too.

-- ── Organization settings ─────────────────────────────────────────────────

create table public.organization_cloud_settings (
  org_id uuid primary key references public.organizations (id) on delete cascade,
  allow_managed boolean not null default false,
  allow_byos boolean not null default false,
  org_cap_usd numeric(10, 2) not null default 500 check (org_cap_usd >= 5),
  member_cap_usd numeric(10, 2) not null default 50 check (member_cap_usd >= 5),
  updated_at timestamptz not null default now()
);
alter table public.organization_cloud_settings enable row level security;
revoke all on public.organization_cloud_settings from public, anon, authenticated;

-- ── Profiles ──────────────────────────────────────────────────────────────

-- null org_id is the personal profile. A profile for an organization the
-- user has left reads as personal (user_billing_org).
create table public.user_cloud_profiles (
  user_id uuid primary key references auth.users (id) on delete cascade,
  org_id uuid references public.organizations (id) on delete set null,
  updated_at timestamptz not null default now()
);
alter table public.user_cloud_profiles enable row level security;
revoke all on public.user_cloud_profiles from public, anon, authenticated;

-- ── Billing owner ─────────────────────────────────────────────────────────

-- null is the owning user. No foreign key: usage stays attributed to an
-- organization after it is deleted, and its workspaces then lapse.
alter table public.remote_instances add column billing_org_id uuid;
alter table public.remote_instance_placements add column billing_org_id uuid;
alter table public.usage_intervals add column billing_org_id uuid;
alter table public.usage_reports add column billing_org_id uuid;
create index remote_instances_billing_org_id_idx on public.remote_instances (billing_org_id)
  where billing_org_id is not null;
create index usage_intervals_billing_org_time on public.usage_intervals (billing_org_id, started_at)
  where billing_org_id is not null;

-- A placement bills where its instance bills; an interval where its
-- placement does. Neither changes afterwards.
create function treq_internal.placement_billing_org()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if tg_op = 'INSERT' then
    new.billing_org_id := (select i.billing_org_id from public.remote_instances i where i.id = new.instance_id);
  elsif new.billing_org_id is distinct from old.billing_org_id then
    raise exception 'billing_org_immutable';
  end if;
  return new;
end;
$$;

-- Named to sort before remote_instance_placements_byos_guard, which reads it.
create trigger remote_instance_placements_billing_org
  before insert or update of billing_org_id on public.remote_instance_placements
  for each row execute function treq_internal.placement_billing_org();

create function treq_internal.usage_interval_billing_org()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if tg_op = 'INSERT' then
    new.billing_org_id := (select p.billing_org_id from public.remote_instance_placements p where p.id = new.placement_id);
  elsif new.billing_org_id is distinct from old.billing_org_id then
    raise exception 'usage_intervals is append and close only';
  end if;
  return new;
end;
$$;

create trigger usage_intervals_billing_org
  before insert or update of billing_org_id on public.usage_intervals
  for each row execute function treq_internal.usage_interval_billing_org();

-- ── Organization keys ─────────────────────────────────────────────────────

-- An organization key belongs to the organization, not to the owner who
-- saved it, so it outlives that owner's account.
alter table public.remote_provider_credentials
  add column owner_org_id uuid references public.organizations (id) on delete cascade,
  alter column owner_user_id drop not null,
  drop constraint remote_provider_credentials_owner_user_id_provider_kind_key,
  add constraint remote_provider_credentials_one_owner check ((owner_user_id is null) <> (owner_org_id is null));
create unique index remote_provider_credentials_personal_kind
  on public.remote_provider_credentials (owner_user_id, provider_kind) where owner_org_id is null;
create unique index remote_provider_credentials_org_kind
  on public.remote_provider_credentials (owner_org_id, provider_kind) where owner_org_id is not null;

-- ── Access ────────────────────────────────────────────────────────────────

-- Personal Pro only. user_has_pro also counts Team membership, which pays for
-- the organization's cloud, not for a member's personal cloud.
create function treq_internal.user_has_personal_pro(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1 from public.billing_subscriptions s
     where s.owner_type = 'user' and s.owner_id = p_user_id and s.plan = 'pro'
       and treq_internal.subscription_entitles(s.status, s.current_period_end));
$$;

-- A personal BYOS key needs personal Pro.
create or replace function treq_internal.user_can_byos(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select treq_internal.user_has_personal_pro(p_user_id);
$$;

-- {entitled, allow_managed, allow_byos} for a user in a billing context:
-- personal (p_org_id null) or an organization.
create function treq_internal.cloud_access(p_user_id uuid, p_org_id uuid)
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
  select case
    when p_org_id is null then jsonb_build_object(
      'entitled', treq_internal.user_has_personal_pro(p_user_id),
      'allow_managed', treq_internal.user_has_personal_pro(p_user_id),
      'allow_byos', treq_internal.user_can_byos(p_user_id))
    else (
      select jsonb_build_object(
        'entitled', coalesce(e.ok and (s.allow_managed or s.allow_byos), false),
        'allow_managed', coalesce(e.ok and s.allow_managed, false),
        'allow_byos', coalesce(e.ok and s.allow_byos, false))
        from (select exists (
                       select 1 from public.organization_members m
                        where m.org_id = p_org_id and m.user_id = p_user_id)
                     and treq_internal.organization_has_team(p_org_id) as ok) e
        left join public.organization_cloud_settings s on s.org_id = p_org_id)
  end;
$$;

create function treq_internal.instance_cloud_entitled(p_user_id uuid, p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select (treq_internal.cloud_access(p_user_id, p_org_id)->>'entitled')::boolean;
$$;

-- Where the user's cloud bills: their live workspace's organization, or with
-- no workspace the organization of their profile (if still a member).
create function treq_internal.user_billing_org(p_user_id uuid)
returns uuid
language sql
stable
security definer
set search_path = ''
as $$
  select case
    when exists (select 1 from public.remote_instances i
                  where i.owner_user_id = p_user_id and i.status <> 'deleted')
      then (select i.billing_org_id from public.remote_instances i
             where i.owner_user_id = p_user_id and i.status <> 'deleted'
             order by i.created_at desc limit 1)
    else (select p.org_id from public.user_cloud_profiles p
           where p.user_id = p_user_id
             and exists (select 1 from public.organization_members m
                          where m.org_id = p.org_id and m.user_id = p_user_id))
  end;
$$;

-- {billing_org_id, entitled, allow_managed, allow_byos} for remote-instance.
create function public.cloud_context_for(p_user_id uuid)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  v_org uuid;
begin
  perform treq_internal.require_service_role();
  v_org := treq_internal.user_billing_org(p_user_id);
  return jsonb_build_object('billing_org_id', v_org) || treq_internal.cloud_access(p_user_id, v_org);
end;
$$;

-- Same contract as has_pro: the caller about themselves, or the service role.
create function public.has_cloud(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (p_user_id = (select auth.uid()) or current_setting('role', true) = 'service_role')
    and (treq_internal.cloud_access(p_user_id, treq_internal.user_billing_org(p_user_id))->>'entitled')::boolean,
    false);
$$;

-- ── Settings and profile RPCs ─────────────────────────────────────────────

create function treq_internal.org_spend_cap_max_usd()
returns numeric
language sql
immutable
set search_path = ''
as $$
  select 10000::numeric;
$$;

create function treq_internal.organization_cloud_settings_json(p_org_id uuid)
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
  select jsonb_build_object(
    'org_id', p_org_id,
    'allow_managed', coalesce(s.allow_managed, false),
    'allow_byos', coalesce(s.allow_byos, false),
    'org_cap_usd', coalesce(s.org_cap_usd, 500),
    'member_cap_usd', coalesce(s.member_cap_usd, 50))
    from (select 1) _
    left join public.organization_cloud_settings s on s.org_id = p_org_id;
$$;

create function public.get_organization_cloud_settings(p_org_id uuid)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  if not exists (select 1 from public.organization_members m
                  where m.org_id = p_org_id and m.user_id = (select auth.uid())) then
    raise exception 'not_organization_member';
  end if;
  return treq_internal.organization_cloud_settings_json(p_org_id);
end;
$$;

create function public.set_organization_cloud_settings(
  p_org_id uuid, p_allow_managed boolean, p_allow_byos boolean,
  p_org_cap_usd numeric, p_member_cap_usd numeric)
returns jsonb
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
  if not exists (select 1 from public.organization_members m
                  where m.org_id = p_org_id and m.user_id = v_user and m.role = 'owner') then
    raise exception 'not_organization_owner';
  end if;
  if p_org_cap_usd is null or p_org_cap_usd < 5 or p_member_cap_usd is null or p_member_cap_usd < 5 then
    raise exception 'spend_cap_too_low';
  end if;
  if p_org_cap_usd > treq_internal.org_spend_cap_max_usd()
     or p_member_cap_usd > treq_internal.org_spend_cap_max_usd() then
    raise exception 'spend_cap_too_high';
  end if;
  insert into public.organization_cloud_settings
    (org_id, allow_managed, allow_byos, org_cap_usd, member_cap_usd, updated_at)
  values (p_org_id, coalesce(p_allow_managed, false), coalesce(p_allow_byos, false),
          round(p_org_cap_usd, 2), round(p_member_cap_usd, 2), now())
  on conflict (org_id) do update set
    allow_managed = excluded.allow_managed, allow_byos = excluded.allow_byos,
    org_cap_usd = excluded.org_cap_usd, member_cap_usd = excluded.member_cap_usd,
    updated_at = excluded.updated_at;
  return treq_internal.organization_cloud_settings_json(p_org_id);
end;
$$;

-- {org_id, entitled, allow_managed, allow_byos} for the caller's profile.
create function public.get_cloud_profile()
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  v_user uuid := (select auth.uid());
  v_org uuid;
begin
  if v_user is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  select p.org_id into v_org from public.user_cloud_profiles p
   where p.user_id = v_user
     and exists (select 1 from public.organization_members m where m.org_id = p.org_id and m.user_id = v_user);
  return jsonb_build_object('org_id', v_org) || treq_internal.cloud_access(v_user, v_org);
end;
$$;

-- Switches the profile new cloud workspaces bill to. A workspace keeps the
-- billing it was created with.
create function public.set_cloud_profile(p_org_id uuid)
returns jsonb
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
  if p_org_id is not null and not exists (
    select 1 from public.organization_members m where m.org_id = p_org_id and m.user_id = v_user
  ) then
    raise exception 'not_organization_member';
  end if;
  insert into public.user_cloud_profiles (user_id, org_id, updated_at)
  values (v_user, p_org_id, now())
  on conflict (user_id) do update set org_id = excluded.org_id, updated_at = excluded.updated_at;
  return public.get_cloud_profile();
end;
$$;

-- The caller's organizations for the desktop: role, Team, and cloud settings.
create function public.list_my_cloud_organizations()
returns table (org_id uuid, name text, role text, has_team boolean, allow_managed boolean,
               allow_byos boolean, org_cap_usd numeric, member_cap_usd numeric)
language sql
stable
security definer
set search_path = ''
as $$
  select o.id, o.name, m.role, treq_internal.organization_has_team(o.id),
         coalesce(s.allow_managed, false), coalesce(s.allow_byos, false),
         coalesce(s.org_cap_usd, 500), coalesce(s.member_cap_usd, 50)
    from public.organization_members m
    join public.organizations o on o.id = m.org_id
    left join public.organization_cloud_settings s on s.org_id = o.id
   where m.user_id = (select auth.uid())
   order by o.name, o.id;
$$;

-- ── Organization key RPCs ─────────────────────────────────────────────────

-- Stores an organization key the caller (remote-instance) already validated
-- with the provider. Only an owner of an organization with Team may.
create function public.provider_credential_save_org(
  p_org_id uuid, p_user_id uuid, p_kind text, p_key text, p_label text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_row public.remote_provider_credentials;
begin
  perform treq_internal.require_service_role();
  if not exists (select 1 from public.organization_members m
                  where m.org_id = p_org_id and m.user_id = p_user_id and m.role = 'owner') then
    raise exception 'not_organization_owner';
  end if;
  if not treq_internal.organization_has_team(p_org_id) then
    raise exception 'byos_not_allowed';
  end if;
  if p_kind is null or p_kind = 'local'
     or not exists (select 1 from public.sandbox_providers where kind = p_kind) then
    raise exception 'byos_kind_unsupported';
  end if;
  if p_key is null or length(p_key) = 0 then
    raise exception 'byos_key_required';
  end if;
  perform pg_advisory_xact_lock(hashtextextended('treq-byos-org:' || p_org_id || ':' || p_kind, 0));

  select * into v_row from public.remote_provider_credentials
   where owner_org_id = p_org_id and provider_kind = p_kind
   for update;
  if found then
    perform vault.update_secret(v_row.vault_secret_id, p_key);
    update public.remote_provider_credentials
       set label = p_label, validated_at = now(), invalid_at = null
     where id = v_row.id
    returning * into v_row;
  else
    insert into public.remote_provider_credentials
      (owner_user_id, owner_org_id, provider_kind, vault_secret_id, label, validated_at)
    values (null, p_org_id, p_kind,
            vault.create_secret(p_key, 'treq-byos-org:' || p_org_id || ':' || p_kind, 'BYOS organization provider key'),
            p_label, now())
    returning * into v_row;
  end if;
  return jsonb_build_object('id', v_row.id, 'org_id', v_row.owner_org_id, 'provider_kind', v_row.provider_kind,
                            'label', v_row.label, 'validated_at', v_row.validated_at,
                            'invalid_at', v_row.invalid_at, 'created_at', v_row.created_at);
end;
$$;

-- Key metadata for members; never the key.
create function public.list_org_provider_credentials(p_org_id uuid)
returns table (id uuid, provider_kind text, label text, validated_at timestamptz,
               invalid_at timestamptz, created_at timestamptz)
language sql
stable
security definer
set search_path = ''
as $$
  select c.id, c.provider_kind, c.label, c.validated_at, c.invalid_at, c.created_at
    from public.remote_provider_credentials c
   where c.owner_org_id = p_org_id
     and exists (select 1 from public.organization_members m
                  where m.org_id = p_org_id and m.user_id = (select auth.uid()))
   order by c.provider_kind;
$$;

create function public.delete_org_provider_credential(p_org_id uuid, p_kind text)
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
  if not exists (select 1 from public.organization_members m
                  where m.org_id = p_org_id and m.user_id = v_user and m.role = 'owner') then
    raise exception 'not_organization_owner';
  end if;
  perform 1 from public.remote_provider_credentials
    where owner_org_id = p_org_id and provider_kind = p_kind
    for update;
  if exists (
    select 1 from public.remote_instance_placements p
      join public.remote_provider_credentials c on c.id = p.credential_id
     where c.owner_org_id = p_org_id and c.provider_kind = p_kind
       and (p.role <> 'dead' or (p.provider_resource_id is not null and p.state <> 'deleted'))
  ) then
    raise exception 'credential_in_use';
  end if;
  delete from public.remote_provider_credentials
   where owner_org_id = p_org_id and provider_kind = p_kind;
end;
$$;

-- ── Spend in an organization ──────────────────────────────────────────────

-- The organization's live Team period from its first full hour, else the
-- UTC calendar month (as spend_window_start for a user).
create function treq_internal.org_spend_window_start(p_org_id uuid, p_now timestamptz)
returns timestamptz
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (select date_trunc('hour', s.current_period_start + interval '1 hour' - interval '1 microsecond')
       from public.billing_subscriptions s
      where s.owner_type = 'organization' and s.owner_id = p_org_id
        and s.plan = 'team'
        and s.status in ('trialing', 'active', 'past_due')
        and s.current_period_start <= p_now
        and s.current_period_end > p_now
      order by s.current_period_start desc
      limit 1),
    date_trunc('month', p_now, 'UTC'));
$$;

-- What treq bills the organization in its window, for one member or (null)
-- for everyone, priced from the ledger.
create function treq_internal.org_spend_nanousd(p_org_id uuid, p_user_id uuid, p_now timestamptz)
returns bigint
language sql
stable
security definer
set search_path = ''
as $$
  with w as (select treq_internal.org_spend_window_start(p_org_id, p_now) as start)
  select coalesce(round(sum(treq_internal.billed_nanousd(
           extract(epoch from least(coalesce(u.ended_at, p_now), p_now) - greatest(u.started_at, w.start))
           * case u.kind
               when 'running' then p.running_nanousd_per_sec
               else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
             end,
           p.markup_bps, u.billing_mode))), 0)::bigint
    from w
    join public.usage_intervals u
      on u.billing_org_id = p_org_id
     and (p_user_id is null or u.owner_user_id = p_user_id)
     and u.started_at < p_now
     and coalesce(u.ended_at, p_now) > w.start
    join public.machine_type_prices p on p.id = u.price_id;
$$;

-- The spend status for wherever the user's cloud bills. In an organization
-- the member is capped when either their own cap or the organization's is
-- reached; cap_usd and spent_usd are the member's.
create function treq_internal.cloud_spend_status(p_user_id uuid, p_now timestamptz)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  v_org uuid := treq_internal.user_billing_org(p_user_id);
  v_settings jsonb;
  v_member numeric;
  v_all numeric;
  v_member_cap numeric;
  v_org_cap numeric;
begin
  if v_org is null then
    return treq_internal.spend_status(p_user_id, p_now) || jsonb_build_object('scope', 'personal');
  end if;
  v_settings := treq_internal.organization_cloud_settings_json(v_org);
  v_member_cap := (v_settings->>'member_cap_usd')::numeric;
  v_org_cap := (v_settings->>'org_cap_usd')::numeric;
  v_member := round(treq_internal.org_spend_nanousd(v_org, p_user_id, p_now) / 1e9, 6);
  v_all := round(treq_internal.org_spend_nanousd(v_org, null, p_now) / 1e9, 6);
  return jsonb_build_object(
    'scope', 'organization',
    'org_id', v_org,
    'cap_usd', v_member_cap,
    'spent_usd', v_member,
    'org_cap_usd', v_org_cap,
    'org_spent_usd', v_all,
    'month_start', treq_internal.org_spend_window_start(v_org, p_now),
    'state', case when v_member >= v_member_cap or v_all >= v_org_cap then 'capped'
                  when v_member >= v_member_cap * 0.8 or v_all >= v_org_cap * 0.8 then 'warning'
                  else 'ok' end);
end;
$$;

-- ── Functions changed for the billing owner ───────────────────────────────


-- A personal workspace uses its owner's personal key; an organization
-- workspace uses that organization's key.
CREATE OR REPLACE FUNCTION treq_internal.placements_byos_guard()
 RETURNS trigger
 LANGUAGE plpgsql
 SET search_path TO ''
AS $function$
begin
  -- On update a null credential is allowed: deleting a credential sets it
  -- null on dead placements (on delete set null).
  if tg_op = 'INSERT' and new.credential_source = 'byos' and new.credential_id is null then
    raise exception 'byos_credential_required';
  end if;
  if new.credential_source = 'byos' and new.credential_id is not null then
    -- Pairs with the row lock in delete_provider_credential().
    perform 1 from public.remote_provider_credentials c
      where c.id = new.credential_id
        and c.provider_kind = new.provider_kind
        and ((new.billing_org_id is null and c.owner_org_id is null and c.owner_user_id = new.owner_user_id)
             or (new.billing_org_id is not null and c.owner_org_id = new.billing_org_id))
      for key share;
    if not found then
      raise exception 'byos_credential_mismatch';
    end if;
  end if;
  if new.credential_source = 'managed' and exists (
    select 1 from public.remote_instance_placements p
     where p.instance_id = new.instance_id
       and p.role = 'active'
       and p.credential_source = 'byos'
  ) then
    raise exception 'byos_placement_must_stay_byos';
  end if;
  return new;
end;
$function$
;

-- Usage on an organization profile is reported apart from the member's
-- personal usage, and billed to the organization (usage-report.ts).
CREATE OR REPLACE FUNCTION public.build_usage_reports(p_until timestamp with time zone)
 RETURNS integer
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_count int;
  v_cutoff timestamptz := date_trunc('hour', p_until - interval '5 minutes');
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  insert into public.usage_reports
    (identifier, owner_user_id, billing_org_id, hour, seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd)
  select b.owner_user_id::text || coalesce(':' || b.billing_org_id::text, '') || ':' || to_char(b.hour at time zone 'UTC', 'YYYY-MM-DD"T"HH24'),
         b.owner_user_id, b.billing_org_id, b.hour,
         round(sum(b.seconds))::int,
         round(sum(b.cost))::bigint,
         round(sum(treq_internal.billed_nanousd(b.cost, b.markup_bps, b.billing_mode)))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then treq_internal.billed_nanousd(b.cost, b.markup_bps, b.billing_mode)
                        else 0 end))::bigint
    from (
      select u.owner_user_id, u.billing_org_id, h.hour, p.markup_bps, u.billing_mode,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour)) as seconds,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as cost
        from public.usage_intervals u
        join public.machine_type_prices p on p.id = u.price_id
        cross join lateral generate_series(
          date_trunc('hour', u.started_at),
          least(coalesce(u.ended_at, v_cutoff), v_cutoff) - interval '1 microsecond',
          interval '1 hour') as h(hour)
       where u.started_at < v_cutoff
    ) b
   where b.hour < v_cutoff and b.seconds > 0
   group by b.owner_user_id, b.billing_org_id, b.hour
  on conflict (identifier) do nothing;
  get diagnostics v_count = row_count;
  return v_count;
end;
$function$
;

-- A personal cap counts personal usage only.
CREATE OR REPLACE FUNCTION treq_internal.month_spend_nanousd(p_user_id uuid, p_now timestamp with time zone)
 RETURNS bigint
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  with bounds as (
    select treq_internal.spend_window_start(p_user_id, p_now) as month_start
  ),
  reported as (
    select coalesce(sum(r.billed_nanousd), 0) as billed,
           max(r.hour) + interval '1 hour' as reported_until
      from public.usage_reports r, bounds b
     where r.owner_user_id = p_user_id
       and r.billing_org_id is null
       and r.hour >= b.month_start and r.hour < p_now
  ),
  open as (
    select coalesce(sum(treq_internal.billed_nanousd(
             extract(epoch from
               least(coalesce(u.ended_at, p_now), p_now)
               - greatest(u.started_at, greatest(b.month_start, coalesce(rp.reported_until, b.month_start))))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end,
             p.markup_bps, u.billing_mode)), 0) as billed
      from bounds b
     cross join reported rp
      join public.usage_intervals u
        on u.owner_user_id = p_user_id
       and u.billing_org_id is null
       and u.started_at < p_now
       and coalesce(u.ended_at, p_now) > greatest(b.month_start, coalesce(rp.reported_until, b.month_start))
      join public.machine_type_prices p on p.id = u.price_id
  )
  select (round((select billed from reported) + (select billed from open)))::bigint;
$function$
;

CREATE OR REPLACE FUNCTION public.get_spend_status()
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
begin
  if (select auth.uid()) is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  return treq_internal.cloud_spend_status((select auth.uid()), now());
end;
$function$
;

CREATE OR REPLACE FUNCTION public.get_spend_status_for(p_user_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
begin
  perform treq_internal.require_service_role();
  return treq_internal.cloud_spend_status(p_user_id, now());
end;
$function$
;

-- Enforcement measures each user in the context their workspace bills to.
CREATE OR REPLACE FUNCTION public.spend_cap_candidates(p_now timestamp with time zone)
 RETURNS TABLE(user_id uuid, cap_usd numeric, spent_usd numeric, month_start timestamp with time zone, state text)
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
begin
  perform treq_internal.require_service_role();
  return query
  select o.owner_user_id,
         (st->>'cap_usd')::numeric, (st->>'spent_usd')::numeric,
         (st->>'month_start')::timestamptz, st->>'state'
    from (
      select distinct u.owner_user_id
        from public.usage_intervals u
       where coalesce(u.ended_at, p_now) >= least(date_trunc('month', p_now, 'UTC'), p_now - interval '32 days')
    ) o
   cross join lateral treq_internal.cloud_spend_status(o.owner_user_id, p_now) st
   where st->>'state' <> 'ok';
end;
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
    select treq_internal.instance_cloud_entitled(i.owner_user_id, i.billing_org_id) as has_pro
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
        when treq_internal.instance_cloud_entitled(i.owner_user_id, i.billing_org_id) then null
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
      and not treq_internal.instance_cloud_entitled(i.owner_user_id, i.billing_org_id)
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
    and treq_internal.instance_cloud_entitled(r.owner_user_id, r.billing_org_id);
  return null;
end;
$function$
;

CREATE OR REPLACE FUNCTION public.provider_credential_save(p_user_id uuid, p_kind text, p_key text, p_label text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_row public.remote_provider_credentials;
begin
  perform treq_internal.require_service_role();
  if not treq_internal.user_can_byos(p_user_id) then
    raise exception 'byos_not_allowed';
  end if;
  if p_kind is null or p_kind = 'local'
     or not exists (select 1 from public.sandbox_providers where kind = p_kind) then
    raise exception 'byos_kind_unsupported';
  end if;
  if p_key is null or length(p_key) = 0 then
    raise exception 'byos_key_required';
  end if;
  -- With no row yet, `for update` below locks nothing: two first saves
  -- would both insert and the loser fail on Vault's unique secret name.
  perform pg_advisory_xact_lock(hashtextextended('treq-byos:' || p_user_id || ':' || p_kind, 0));

  select * into v_row from public.remote_provider_credentials
   where owner_user_id = p_user_id and provider_kind = p_kind and owner_org_id is null
   for update;
  if found then
    perform vault.update_secret(v_row.vault_secret_id, p_key);
    update public.remote_provider_credentials
       set label = p_label, validated_at = now(), invalid_at = null
     where id = v_row.id
    returning * into v_row;
  else
    insert into public.remote_provider_credentials
      (owner_user_id, provider_kind, vault_secret_id, label, validated_at)
    values (p_user_id, p_kind,
            vault.create_secret(p_key, 'treq-byos:' || p_user_id || ':' || p_kind,
                                'BYOS provider key'),
            p_label, now())
    returning * into v_row;
  end if;
  return jsonb_build_object('id', v_row.id, 'provider_kind', v_row.provider_kind, 'label', v_row.label,
                            'validated_at', v_row.validated_at, 'invalid_at', v_row.invalid_at,
                            'created_at', v_row.created_at);
end;
$function$
;

CREATE OR REPLACE FUNCTION public.list_provider_credentials()
 RETURNS TABLE(id uuid, provider_kind text, label text, validated_at timestamp with time zone, invalid_at timestamp with time zone, created_at timestamp with time zone)
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  select c.id, c.provider_kind, c.label, c.validated_at, c.invalid_at, c.created_at
    from public.remote_provider_credentials c
   where c.owner_user_id = (select auth.uid())
     and c.owner_org_id is null
   order by c.provider_kind;
$function$
;

CREATE OR REPLACE FUNCTION public.delete_provider_credential(p_kind text)
 RETURNS void
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_user uuid := (select auth.uid());
begin
  if v_user is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  -- Lock first: a concurrent placement insert holds or waits on a key share
  -- lock on this row, so the check below sees its placement.
  perform 1 from public.remote_provider_credentials
    where owner_user_id = v_user and provider_kind = p_kind and owner_org_id is null
    for update;
  if exists (
    select 1 from public.remote_instance_placements p
      join public.remote_provider_credentials c on c.id = p.credential_id
     where c.owner_user_id = v_user and c.provider_kind = p_kind and c.owner_org_id is null and (p.role <> 'dead'
            or (p.provider_resource_id is not null and p.state <> 'deleted'))
  ) then
    raise exception 'credential_in_use';
  end if;
  delete from public.remote_provider_credentials
   where owner_user_id = v_user and provider_kind = p_kind and owner_org_id is null;
end;
$function$
;

CREATE OR REPLACE FUNCTION public.get_byos_catalog()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
           'provider_kind', c.provider_kind,
           'provider_name', sp.display_name,
           'credential_valid', c.invalid_at is null,
           'tier_id', t.tier_id,
           'machine_type_id', t.id,
           'regions', to_jsonb(t.regions),
           'provider_hourly_usd', round(p.running_nanousd_per_sec * 3600 / 1e9, 6),
           'hourly_usd', round(treq_internal.billed_nanousd(
              p.running_nanousd_per_sec * 3600, p.markup_bps, 'byos') / 1e9, 6)
         ) order by c.provider_kind, st.sort), '[]'::jsonb)
    from public.remote_provider_credentials c
    join public.sandbox_providers sp on sp.kind = c.provider_kind
    join lateral (
      select distinct on (m.tier_id) m.*
        from public.provider_machine_types m
       where m.provider_kind = c.provider_kind
       order by m.tier_id, m.selectable desc, m.id
    ) t on true
    join public.size_tiers st on st.id = t.tier_id
    join public.machine_type_prices p on p.id = treq_internal.current_price_id(t.id, now())
   where c.owner_user_id = (select auth.uid())
     and c.owner_org_id is null;
$function$
;

-- ── Grants ────────────────────────────────────────────────────────────────

revoke all on function treq_internal.placement_billing_org() from public, anon, authenticated;
revoke all on function treq_internal.usage_interval_billing_org() from public, anon, authenticated;
revoke all on function treq_internal.user_has_personal_pro(uuid) from public, anon, authenticated, service_role;
revoke all on function treq_internal.user_can_byos(uuid) from public, anon, authenticated, service_role;
revoke all on function treq_internal.cloud_access(uuid, uuid) from public, anon, authenticated, service_role;
revoke all on function treq_internal.instance_cloud_entitled(uuid, uuid) from public, anon, authenticated, service_role;
revoke all on function treq_internal.user_billing_org(uuid) from public, anon, authenticated, service_role;
revoke all on function treq_internal.org_spend_cap_max_usd() from public, anon, authenticated;
revoke all on function treq_internal.organization_cloud_settings_json(uuid) from public, anon, authenticated;
revoke all on function treq_internal.org_spend_window_start(uuid, timestamptz) from public, anon, authenticated;
revoke all on function treq_internal.org_spend_nanousd(uuid, uuid, timestamptz) from public, anon, authenticated;
revoke all on function treq_internal.cloud_spend_status(uuid, timestamptz) from public, anon, authenticated;

revoke all on function public.cloud_context_for(uuid) from public, anon, authenticated;
grant execute on function public.cloud_context_for(uuid) to service_role;
revoke all on function public.has_cloud(uuid) from public, anon;
grant execute on function public.has_cloud(uuid) to authenticated, service_role;
revoke all on function public.provider_credential_save_org(uuid, uuid, text, text, text) from public, anon, authenticated;
grant execute on function public.provider_credential_save_org(uuid, uuid, text, text, text) to service_role;

revoke all on function public.get_organization_cloud_settings(uuid) from public, anon;
revoke all on function public.set_organization_cloud_settings(uuid, boolean, boolean, numeric, numeric) from public, anon;
revoke all on function public.get_cloud_profile() from public, anon;
revoke all on function public.list_my_cloud_organizations() from public, anon;
revoke all on function public.set_cloud_profile(uuid) from public, anon;
revoke all on function public.list_org_provider_credentials(uuid) from public, anon;
revoke all on function public.delete_org_provider_credential(uuid, text) from public, anon;
grant execute on function public.get_organization_cloud_settings(uuid) to authenticated;
grant execute on function public.set_organization_cloud_settings(uuid, boolean, boolean, numeric, numeric) to authenticated;
grant execute on function public.get_cloud_profile() to authenticated;
grant execute on function public.list_my_cloud_organizations() to authenticated;
grant execute on function public.set_cloud_profile(uuid) to authenticated;
grant execute on function public.list_org_provider_credentials(uuid) to authenticated;
grant execute on function public.delete_org_provider_credential(uuid, text) to authenticated;

