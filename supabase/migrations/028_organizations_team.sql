-- Organizations, Team entitlement and shared GitHub App installations
-- (prds/billing-and-teams.md, "Organizations and seats", rollout step 4).
--
-- An organization owns a Team subscription and, once an owner attaches it,
-- a GitHub App installation. Team is a flat price for up to 10 members, and
-- pending invites count toward the 10. An invite is matched by a
-- single-use token that expires after 7 days, never by email: the email is
-- only shown to the owner. Only the token's SHA-256 is stored.
--
-- Clients only read these tables. Every write goes through a security
-- definer function below that only the service role can call, so the
-- `organizations` Edge Function stays a thin wrapper and pgTAP covers each
-- rule. Refusals raise SQLSTATE PTnnn, which PostgREST and the Edge
-- Function answer with HTTP status nnn. The hint carries a stable code.

-- ── Tables ────────────────────────────────────────────────────────────────

create table public.organizations (
  id uuid primary key default gen_random_uuid(),
  name text not null check (char_length(name) between 1 and 100 and name = btrim(name)),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table public.organization_members (
  org_id uuid not null references public.organizations(id) on delete cascade,
  user_id uuid not null references auth.users(id) on delete cascade,
  role text not null check (role in ('owner', 'member')),
  created_at timestamptz not null default now(),
  primary key (org_id, user_id)
);

create index idx_organization_members_user on public.organization_members (user_id);

create table public.organization_invites (
  id uuid primary key default gen_random_uuid(),
  org_id uuid not null references public.organizations(id) on delete cascade,
  -- Lowercased. Shown to the owner. Never used to match the invite.
  email text not null check (char_length(email) between 3 and 320),
  token_hash text not null unique,
  invited_by uuid references auth.users(id) on delete set null,
  created_at timestamptz not null default now(),
  expires_at timestamptz not null,
  accepted_at timestamptz,
  accepted_by uuid references auth.users(id) on delete set null,
  revoked_at timestamptz
);

create index idx_organization_invites_org on public.organization_invites (org_id)
  where accepted_at is null and revoked_at is null;

-- Null for a personal installation. Set when an owner attaches it, after
-- which only the organization's owners can relink it.
alter table public.github_app_installations
  add column organization_id uuid references public.organizations(id) on delete set null;

create index idx_github_app_installations_org on public.github_app_installations (organization_id);

-- ── Membership checks ─────────────────────────────────────────────────────

-- For RLS policies: does the signed-in user belong to (or own) this
-- organization? They answer only about auth.uid(), so they reveal nothing a
-- user could not read anyway. Security definer, so the policies on
-- organization_members can use them without recursing into themselves.
create function public.is_organization_member(p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1 from public.organization_members m
    where m.org_id = p_org_id and m.user_id = (select auth.uid())
  );
$$;

create function public.is_organization_owner(p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1 from public.organization_members m
    where m.org_id = p_org_id and m.user_id = (select auth.uid()) and m.role = 'owner'
  );
$$;

revoke all on function public.is_organization_member(uuid) from public, anon;
revoke all on function public.is_organization_owner(uuid) from public, anon;
grant execute on function public.is_organization_member(uuid) to authenticated, service_role;
grant execute on function public.is_organization_owner(uuid) to authenticated, service_role;

create function treq_internal.organization_role(p_org_id uuid, p_user_id uuid)
returns text
language sql
stable
security definer
set search_path = ''
as $$
  select m.role from public.organization_members m
  where m.org_id = p_org_id and m.user_id = p_user_id;
$$;

-- Raises unless p_actor owns the organization. A missing organization and
-- one the actor does not own look the same.
create function treq_internal.require_organization_owner(p_org_id uuid, p_actor uuid)
returns void
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  if treq_internal.organization_role(p_org_id, p_actor) is distinct from 'owner' then
    raise exception 'Only an owner of this organization can do that.'
      using errcode = 'PT403', hint = 'owner_required';
  end if;
end;
$$;

-- Members plus pending invites: what the 10 seats of a Team cover.
create function treq_internal.organization_seats_used(p_org_id uuid)
returns int
language sql
stable
security definer
set search_path = ''
as $$
  select
    (select count(*) from public.organization_members m where m.org_id = p_org_id)::int
    + (select count(*) from public.organization_invites v
       where v.org_id = p_org_id
         and v.accepted_at is null
         and v.revoked_at is null
         and v.expires_at > now())::int;
$$;

revoke all on function treq_internal.organization_role(uuid, uuid)
  from public, anon, authenticated, service_role;
revoke all on function treq_internal.require_organization_owner(uuid, uuid)
  from public, anon, authenticated, service_role;
revoke all on function treq_internal.organization_seats_used(uuid)
  from public, anon, authenticated, service_role;

-- ── Row-level security ────────────────────────────────────────────────────

alter table public.organizations enable row level security;
alter table public.organization_members enable row level security;
alter table public.organization_invites enable row level security;

create policy "Members can view their organizations"
  on public.organizations for select
  to authenticated
  using (public.is_organization_member(id));

create policy "Members can view their organization's members"
  on public.organization_members for select
  to authenticated
  using (public.is_organization_member(org_id));

create policy "Owners can view their organization's invites"
  on public.organization_invites for select
  to authenticated
  using (public.is_organization_owner(org_id));

-- 008_public_grants.sql gives anon and authenticated full DML on every new
-- public table. Membership decides entitlement, so clients only read, and
-- nobody can promote themselves. Clients never see a token hash.
revoke all on table
  public.organizations,
  public.organization_members,
  public.organization_invites
from anon, authenticated;

grant select on table public.organizations, public.organization_members to authenticated;
grant select (id, org_id, email, invited_by, created_at, expires_at, accepted_at, accepted_by, revoked_at)
  on table public.organization_invites to authenticated;

grant select, insert, update, delete on table
  public.organizations,
  public.organization_members,
  public.organization_invites
to service_role;

-- The member list shows names and emails.
create policy "Members can view the profiles of their organization's members"
  on public.profiles for select
  to authenticated
  using (
    exists (
      select 1
      from public.organization_members mine
      join public.organization_members theirs on theirs.org_id = mine.org_id
      where mine.user_id = (select auth.uid())
        and theirs.user_id = profiles.id
    )
  );

-- Members read the organization's installation, its repositories and their
-- merge queue configuration. Writes stay with the user who linked the
-- installation, who is an owner (see organization_attach_installation and
-- github_link_installation).
create policy "Members can view their organization's installations"
  on public.github_app_installations for select
  to authenticated
  using (organization_id is not null and public.is_organization_member(organization_id));

create policy "Members can view their organization's repositories"
  on public.github_repositories for select
  to authenticated
  using (
    exists (
      select 1 from public.github_app_installations i
      where i.id = installation_id
        and i.organization_id is not null
        and public.is_organization_member(i.organization_id)
    )
  );

create policy "Members can view their organization's merge queue configs"
  on public.merge_queue_configs for select
  to authenticated
  using (
    exists (
      select 1
      from public.github_repositories r
      join public.github_app_installations i on i.id = r.installation_id
      where r.id = repo_id
        and i.organization_id is not null
        and public.is_organization_member(i.organization_id)
    )
  );

-- The Team tab shows the organization's subscription to its members and
-- the billing portal button to its owners.
create policy "Members can view their organization's subscriptions"
  on public.billing_subscriptions for select
  to authenticated
  using (owner_type = 'organization' and public.is_organization_member(owner_id));

create policy "Owners can view their organization's billing customer"
  on public.billing_customers for select
  to authenticated
  using (owner_type = 'organization' and public.is_organization_owner(owner_id));

-- ── Entitlement ───────────────────────────────────────────────────────────

create function treq_internal.organization_has_team(p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1
    from public.billing_subscriptions s
    where s.owner_type = 'organization'
      and s.owner_id = p_org_id
      and s.plan = 'team'
      and s.status in ('trialing', 'active', 'past_due')
  );
$$;

revoke all on function treq_internal.organization_has_team(uuid)
  from public, anon, authenticated, service_role;

-- billing-checkout refuses a second Team for an organization that has one.
create function public.organization_has_team(p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select treq_internal.organization_has_team(p_org_id);
$$;

revoke all on function public.organization_has_team(uuid) from public, anon, authenticated;
grant execute on function public.organization_has_team(uuid) to service_role;

-- A user is entitled to Pro through their own Pro subscription or through
-- membership in an organization with Team. Membership is read live, so a
-- member who leaves or is removed loses Team at once (open decision B03).
create or replace function treq_internal.user_has_pro(p_user_id uuid)
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
      and s.status in ('trialing', 'active', 'past_due')
  )
  or exists (
    select 1
    from public.organization_members m
    where m.user_id = p_user_id
      and treq_internal.organization_has_team(m.org_id)
  );
$$;

-- An organization's installation is entitled by the organization's Team. A
-- personal one by the user who linked it.
create or replace function public.installation_has_pro(p_installation_id bigint)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (
      select case
        when i.organization_id is not null
          then treq_internal.organization_has_team(i.organization_id)
        else i.linked_user_id is not null and treq_internal.user_has_pro(i.linked_user_id)
      end
      from public.github_app_installations i
      where i.id = p_installation_id
    ),
    false
  );
$$;

-- Keeps its columns. A Team member reads plan 'pro' with the Team's status
-- and period end. When several subscriptions entitle the user, the one that
-- is not set to cancel wins, then the one that runs longest.
create or replace function public.get_current_user_subscription()
returns table (
  plan text,
  status text,
  current_period_end timestamp
)
language sql
stable
security definer
set search_path = ''
as $$
  with entitled_sub as (
    select s.status, s.current_period_end, s.cancel_at_period_end
    from public.billing_subscriptions s
    where s.status in ('trialing', 'active', 'past_due')
      and (
        (s.owner_type = 'user' and s.owner_id = (select auth.uid()) and s.plan = 'pro')
        or (
          s.owner_type = 'organization'
          and s.plan = 'team'
          and exists (
            select 1 from public.organization_members m
            where m.org_id = s.owner_id and m.user_id = (select auth.uid())
          )
        )
      )
    order by s.cancel_at_period_end, s.current_period_end desc nulls last
    limit 1
  )
  select
    case when public.has_pro((select auth.uid())) then 'pro' else 'free' end as plan,
    case
      when e.status is null then 'inactive'
      when e.cancel_at_period_end then 'canceled'
      else e.status
    end as status,
    (e.current_period_end at time zone 'utc') as current_period_end
  from (select 1) as _
  left join entitled_sub e on true;
$$;

-- A subscription that entitles its owner clears the cloud workspace lapse
-- of everyone it covers: the user, or each member of the organization.
create or replace function treq_internal.clear_remote_lapse_on_entitlement()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
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
    and treq_internal.user_has_pro(r.owner_user_id);
  return null;
end;
$$;

-- ── Billing owners ────────────────────────────────────────────────────────
-- 025 and 026 reserved owner_type 'organization' for this step.

create or replace function public.billing_record_checkout_completed(
  p_event_id text,
  p_customer_id text,
  p_owner_type text,
  p_owner_id uuid
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_owner_type text;
  v_owner_id uuid;
begin
  if p_owner_type is null or p_owner_type not in ('user', 'organization') then
    raise exception 'unsupported billing owner type %', p_owner_type
      using errcode = 'invalid_parameter_value';
  end if;

  insert into public.billing_events (stripe_event_id, type)
  values (p_event_id, 'checkout.session.completed')
  on conflict (stripe_event_id) do nothing;
  if not found then
    return 'duplicate';
  end if;

  if (p_owner_type = 'user' and not exists (select 1 from auth.users u where u.id = p_owner_id))
    or (p_owner_type = 'organization'
        and not exists (select 1 from public.organizations o where o.id = p_owner_id)) then
    return 'unknown_owner';
  end if;

  insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
  values (p_owner_type, p_owner_id, p_customer_id)
  on conflict do nothing;

  select c.owner_type, c.owner_id
    into v_owner_type, v_owner_id
  from public.billing_customers c
  where c.stripe_customer_id = p_customer_id;

  if v_owner_type is distinct from p_owner_type
    or v_owner_id is distinct from p_owner_id then
    return 'owner_conflict';
  end if;

  update public.billing_subscriptions
  set owner_type = p_owner_type, owner_id = p_owner_id, updated_at = now()
  where stripe_customer_id = p_customer_id
    and owner_id is null;

  update public.billing_customers c
  set trial_used_at = now(), updated_at = now()
  where c.stripe_customer_id = p_customer_id
    and c.trial_used_at is null
    and exists (
      select 1
      from public.billing_subscriptions s
      where s.stripe_customer_id = p_customer_id
        and s.plan = 'pro'
        and s.trial_end is not null
    );

  return 'applied';
end;
$$;

create or replace function public.billing_attach_customer(
  p_owner_type text,
  p_owner_id uuid,
  p_customer_id text
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_customer_id text;
begin
  if p_owner_type is null or p_owner_type not in ('user', 'organization') then
    raise exception 'unsupported billing owner type %', p_owner_type
      using errcode = 'invalid_parameter_value';
  end if;

  if (p_owner_type = 'user' and not exists (select 1 from auth.users u where u.id = p_owner_id))
    or (p_owner_type = 'organization'
        and not exists (select 1 from public.organizations o where o.id = p_owner_id)) then
    raise exception 'unknown billing owner %', p_owner_id;
  end if;

  insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
  values (p_owner_type, p_owner_id, p_customer_id)
  on conflict do nothing;

  select c.stripe_customer_id
    into v_customer_id
  from public.billing_customers c
  where c.owner_type = p_owner_type
    and c.owner_id = p_owner_id;

  if v_customer_id is null then
    raise exception 'Stripe customer % belongs to another owner', p_customer_id;
  end if;

  return v_customer_id;
end;
$$;

-- ── Organization actions (service role only) ──────────────────────────────
--
-- p_actor is the user the organizations Edge Function verified from the
-- JWT. Seat changes lock the organization row, so two invites (or an invite
-- and an accept) cannot both take the last seat.

create function public.organization_create(p_actor uuid, p_name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_name text := btrim(coalesce(p_name, ''));
  v_org public.organizations;
begin
  if char_length(v_name) not between 1 and 100 then
    raise exception 'An organization name has 1 to 100 characters.'
      using errcode = 'PT400', hint = 'invalid_name';
  end if;

  insert into public.organizations (name) values (v_name) returning * into v_org;
  insert into public.organization_members (org_id, user_id, role)
  values (v_org.id, p_actor, 'owner');

  return jsonb_build_object('id', v_org.id, 'name', v_org.name);
end;
$$;

-- Returns the invite and its token. The token is never stored, so this is
-- the only time anyone sees it. Inviting an address that already has a
-- pending invite issues a new token for that invite (the old link stops
-- working) and a fresh 7 days, without taking another seat.
create function public.organization_invite(p_actor uuid, p_org_id uuid, p_email text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_email text := lower(btrim(coalesce(p_email, '')));
  v_token text := encode(extensions.gen_random_bytes(32), 'hex');
  v_invite public.organization_invites;
begin
  perform treq_internal.require_organization_owner(p_org_id, p_actor);

  if char_length(v_email) > 320 or v_email !~ '^[^@[:space:]]+@[^@[:space:]]+\.[^@[:space:]]+$' then
    raise exception 'Enter a valid email address.'
      using errcode = 'PT400', hint = 'invalid_email';
  end if;

  perform 1 from public.organizations o where o.id = p_org_id for update;

  if exists (
    select 1
    from public.organization_members m
    join auth.users u on u.id = m.user_id
    where m.org_id = p_org_id and lower(u.email) = v_email
  ) then
    raise exception 'That address already belongs to a member.'
      using errcode = 'PT409', hint = 'already_member';
  end if;

  update public.organization_invites v
  set token_hash = encode(sha256(convert_to(v_token, 'UTF8')), 'hex'),
      invited_by = p_actor,
      expires_at = now() + interval '7 days'
  where v.org_id = p_org_id
    and v.email = v_email
    and v.accepted_at is null
    and v.revoked_at is null
    and v.expires_at > now()
  returning * into v_invite;

  if not found then
    if treq_internal.organization_seats_used(p_org_id) >= 10 then
      raise exception 'A Team covers 10 members, counting pending invites. Remove a member or revoke an invite first.'
        using errcode = 'PT409', hint = 'seat_limit';
    end if;
    insert into public.organization_invites (org_id, email, token_hash, invited_by, expires_at)
    values (
      p_org_id, v_email, encode(sha256(convert_to(v_token, 'UTF8')), 'hex'), p_actor,
      now() + interval '7 days'
    )
    returning * into v_invite;
  end if;

  return jsonb_build_object(
    'invite_id', v_invite.id,
    'email', v_invite.email,
    'expires_at', v_invite.expires_at,
    'token', v_token
  );
end;
$$;

-- Any signed-in account holding the token joins as a member. The token
-- works once. A user who is already a member uses it up without joining
-- twice, which frees its seat.
create function public.organization_accept_invite(p_actor uuid, p_token text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_hash text;
  v_org_id uuid;
  v_invite public.organization_invites;
  v_org public.organizations;
  v_result text;
begin
  if p_token is null or p_token !~ '^[0-9a-f]{64}$' then
    raise exception 'This invite link is not valid.'
      using errcode = 'PT404', hint = 'invite_not_found';
  end if;
  v_hash := encode(sha256(convert_to(p_token, 'UTF8')), 'hex');

  select v.org_id into v_org_id from public.organization_invites v where v.token_hash = v_hash;
  if v_org_id is null then
    raise exception 'This invite link is not valid.'
      using errcode = 'PT404', hint = 'invite_not_found';
  end if;

  -- Same lock order as organization_invite: the organization, then the invite.
  select * into v_org from public.organizations o where o.id = v_org_id for update;
  select * into v_invite from public.organization_invites v where v.token_hash = v_hash for update;
  if v_invite.id is null then
    raise exception 'This invite link is not valid.'
      using errcode = 'PT404', hint = 'invite_not_found';
  end if;

  if v_invite.accepted_at is not null then
    raise exception 'This invite has already been used.'
      using errcode = 'PT410', hint = 'invite_used';
  end if;
  if v_invite.revoked_at is not null then
    raise exception 'This invite was revoked. Ask an owner for a new link.'
      using errcode = 'PT410', hint = 'invite_revoked';
  end if;
  if v_invite.expires_at <= now() then
    raise exception 'This invite has expired. Ask an owner for a new link.'
      using errcode = 'PT410', hint = 'invite_expired';
  end if;

  if treq_internal.organization_role(v_org.id, p_actor) is not null then
    v_result := 'already_member';
  else
    -- The invite already holds a seat, so this only fails if the count was
    -- broken some other way.
    if (select count(*) from public.organization_members m where m.org_id = v_org.id) >= 10 then
      raise exception 'A Team covers 10 members, counting pending invites. Remove a member or revoke an invite first.'
        using errcode = 'PT409', hint = 'seat_limit';
    end if;
    insert into public.organization_members (org_id, user_id, role)
    values (v_org.id, p_actor, 'member');
    v_result := 'joined';
  end if;

  update public.organization_invites
  set accepted_at = now(), accepted_by = p_actor
  where id = v_invite.id;

  -- Joining a Team restores Pro now, so a cloud workspace lapse recorded
  -- earlier must not count toward its deletion.
  update public.remote_instances r
  set lapsed_at = null, updated_at = now()
  where r.owner_user_id = p_actor
    and r.lapsed_at is not null
    and r.status <> 'deleted'
    and treq_internal.user_has_pro(p_actor);

  return jsonb_build_object('organization_id', v_org.id, 'name', v_org.name, 'result', v_result);
end;
$$;

create function public.organization_revoke_invite(p_actor uuid, p_invite_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_org_id uuid;
begin
  select v.org_id into v_org_id from public.organization_invites v where v.id = p_invite_id;
  if v_org_id is null
    or treq_internal.organization_role(v_org_id, p_actor) is distinct from 'owner' then
    raise exception 'Invite not found.'
      using errcode = 'PT404', hint = 'invite_not_found';
  end if;

  update public.organization_invites v
  set revoked_at = now()
  where v.id = p_invite_id
    and v.accepted_at is null
    and v.revoked_at is null;
  if not found then
    raise exception 'This invite was already accepted or revoked.'
      using errcode = 'PT409', hint = 'invite_not_pending';
  end if;
end;
$$;

-- Removes a member and hands any of the organization's installations they
-- linked to p_successor, so a former member keeps no access through
-- linked_user_id. Raises when the member is the last owner.
create function treq_internal.organization_drop_member(
  p_org_id uuid,
  p_user_id uuid,
  p_successor uuid
)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_role text;
  v_successor uuid := p_successor;
begin
  perform 1 from public.organizations o where o.id = p_org_id for update;

  v_role := treq_internal.organization_role(p_org_id, p_user_id);
  if v_role is null then
    raise exception 'That account is not a member of this organization.'
      using errcode = 'PT404', hint = 'member_not_found';
  end if;
  if v_role = 'owner' and (
    select count(*) from public.organization_members m
    where m.org_id = p_org_id and m.role = 'owner'
  ) <= 1 then
    raise exception 'An organization needs at least one owner.'
      using errcode = 'PT409', hint = 'last_owner';
  end if;

  delete from public.organization_members m
  where m.org_id = p_org_id and m.user_id = p_user_id;

  if v_successor is null or v_successor = p_user_id then
    select m.user_id into v_successor
    from public.organization_members m
    where m.org_id = p_org_id and m.role = 'owner'
    order by m.created_at, m.user_id
    limit 1;
  end if;

  update public.github_app_installations i
  set linked_user_id = v_successor, updated_at = now()
  where i.organization_id = p_org_id
    and i.linked_user_id = p_user_id;
end;
$$;

revoke all on function treq_internal.organization_drop_member(uuid, uuid, uuid)
  from public, anon, authenticated, service_role;

create function public.organization_remove_member(p_actor uuid, p_org_id uuid, p_user_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_organization_owner(p_org_id, p_actor);
  perform treq_internal.organization_drop_member(p_org_id, p_user_id, p_actor);
end;
$$;

create function public.organization_leave(p_actor uuid, p_org_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  if treq_internal.organization_role(p_org_id, p_actor) is null then
    raise exception 'You are not a member of this organization.'
      using errcode = 'PT404', hint = 'not_member';
  end if;
  perform treq_internal.organization_drop_member(p_org_id, p_actor, null);
end;
$$;

-- An owner attaches an installation they linked. From then on the
-- organization's Team entitles it, its members can read it, and only the
-- organization's owners can relink it. Returns 'attached' or
-- 'already_attached'.
create function public.organization_attach_installation(
  p_actor uuid,
  p_org_id uuid,
  p_installation_id bigint
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_installation public.github_app_installations;
begin
  perform treq_internal.require_organization_owner(p_org_id, p_actor);

  select * into v_installation
  from public.github_app_installations i
  where i.id = p_installation_id
  for update;

  if v_installation.id is not null and v_installation.organization_id = p_org_id then
    return 'already_attached';
  end if;
  if v_installation.id is null or v_installation.linked_user_id is distinct from p_actor then
    raise exception 'Link this GitHub App installation to your account first.'
      using errcode = 'PT404', hint = 'installation_not_found';
  end if;
  if v_installation.organization_id is not null then
    raise exception 'This installation already belongs to another organization.'
      using errcode = 'PT409', hint = 'installation_in_other_organization';
  end if;

  update public.github_app_installations i
  set organization_id = p_org_id, updated_at = now()
  where i.id = p_installation_id;
  return 'attached';
end;
$$;

-- complete-github-installation's write. Links the installation to p_actor
-- unless it belongs to an organization p_actor does not own. Returns
-- {"result": "linked", "organization_id": ...} or
-- {"result": "organization_owner_required", "organization_id": null}.
create function public.github_link_installation(
  p_actor uuid,
  p_installation_id bigint,
  p_account_login text,
  p_account_type text,
  p_account_avatar_url text,
  p_app_id int
)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_org_id uuid;
begin
  insert into public.github_app_installations as i (
    id, account_login, account_type, account_avatar_url, app_id, linked_user_id, updated_at
  )
  values (
    p_installation_id, p_account_login, p_account_type, p_account_avatar_url, p_app_id, p_actor, now()
  )
  on conflict (id) do update set
    account_login = excluded.account_login,
    account_type = excluded.account_type,
    account_avatar_url = excluded.account_avatar_url,
    app_id = excluded.app_id,
    linked_user_id = excluded.linked_user_id,
    updated_at = now()
  where i.organization_id is null
    or treq_internal.organization_role(i.organization_id, p_actor) = 'owner'
  returning i.organization_id into v_org_id;

  if not found then
    return jsonb_build_object('result', 'organization_owner_required', 'organization_id', null);
  end if;
  return jsonb_build_object('result', 'linked', 'organization_id', v_org_id);
end;
$$;

revoke all on function public.organization_create(uuid, text) from public, anon, authenticated;
revoke all on function public.organization_invite(uuid, uuid, text) from public, anon, authenticated;
revoke all on function public.organization_accept_invite(uuid, text) from public, anon, authenticated;
revoke all on function public.organization_revoke_invite(uuid, uuid) from public, anon, authenticated;
revoke all on function public.organization_remove_member(uuid, uuid, uuid) from public, anon, authenticated;
revoke all on function public.organization_leave(uuid, uuid) from public, anon, authenticated;
revoke all on function public.organization_attach_installation(uuid, uuid, bigint)
  from public, anon, authenticated;
revoke all on function public.github_link_installation(uuid, bigint, text, text, text, int)
  from public, anon, authenticated;

grant execute on function public.organization_create(uuid, text) to service_role;
grant execute on function public.organization_invite(uuid, uuid, text) to service_role;
grant execute on function public.organization_accept_invite(uuid, text) to service_role;
grant execute on function public.organization_revoke_invite(uuid, uuid) to service_role;
grant execute on function public.organization_remove_member(uuid, uuid, uuid) to service_role;
grant execute on function public.organization_leave(uuid, uuid) to service_role;
grant execute on function public.organization_attach_installation(uuid, uuid, bigint) to service_role;
grant execute on function public.github_link_installation(uuid, bigint, text, text, text, int)
  to service_role;
