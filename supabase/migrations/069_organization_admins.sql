-- Organization admins. An admin manages the team's billing account (Team
-- checkout and Manage billing), invites and revokes invites, and makes
-- members admins or admins members. Only owners make owners, remove
-- members or delete the organization, and an admin never changes an
-- owner's role. Billing belongs to the organization, so any owner or admin
-- manages the same Stripe customer.

alter table public.organization_members
  drop constraint organization_members_role_check,
  add constraint organization_members_role_check
    check (role in ('owner', 'admin', 'member'));

-- Raises unless p_actor is an owner or admin of the organization.
create function treq_internal.require_organization_manager(p_org_id uuid, p_actor uuid)
returns void
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  if coalesce(treq_internal.organization_role(p_org_id, p_actor), '') not in ('owner', 'admin') then
    raise exception 'Only an owner or admin of this organization can do that.'
      using errcode = 'PT403', hint = 'admin_required';
  end if;
end;
$$;

-- Whether the signed-in user manages this organization's billing account.
create function public.can_manage_organization_billing(p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1 from public.organization_members m
    where m.org_id = p_org_id and m.user_id = (select auth.uid())
      and m.role in ('owner', 'admin')
  );
$$;

-- Sets a non-owner's role to 'admin' or 'member'. Returns 'updated', or
-- 'unchanged' when the role was already set.
create function public.organization_set_role(
  p_actor uuid, p_org_id uuid, p_user_id uuid, p_role text
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_role text;
begin
  if p_role is null or p_role not in ('admin', 'member') then
    raise exception 'Role must be admin or member. Use promote to make an owner.'
      using errcode = 'PT400', hint = 'invalid_role';
  end if;
  perform 1 from public.organizations o where o.id = p_org_id for update;
  perform treq_internal.require_organization_manager(p_org_id, p_actor);

  v_role := treq_internal.organization_role(p_org_id, p_user_id);
  if v_role is null then
    raise exception 'That account is not a member of this organization.'
      using errcode = 'PT404', hint = 'member_not_found';
  end if;
  if v_role = 'owner' then
    raise exception 'An owner''s role changes only through demote.'
      using errcode = 'PT403', hint = 'owner_role_locked';
  end if;
  if v_role = p_role then
    return 'unchanged';
  end if;

  update public.organization_members m
  set role = p_role
  where m.org_id = p_org_id and m.user_id = p_user_id;
  return 'updated';
end;
$$;

create or replace function public.organization_invite(p_actor uuid, p_org_id uuid, p_email text)
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
  perform treq_internal.require_organization_manager(p_org_id, p_actor);

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

create or replace function public.organization_revoke_invite(p_actor uuid, p_invite_id uuid)
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
    or treq_internal.organization_role(v_org_id, p_actor) not in ('owner', 'admin')
    or treq_internal.organization_role(v_org_id, p_actor) is null then
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

drop policy "Owners can view their organization's invites" on public.organization_invites;
create policy "Owners and admins can view their organization's invites"
  on public.organization_invites for select
  to authenticated
  using (public.can_manage_organization_billing(org_id));

drop policy "Owners can view their organization's billing customer" on public.billing_customers;
create policy "Owners and admins can view their organization's billing customer"
  on public.billing_customers for select
  to authenticated
  using (owner_type = 'organization' and public.can_manage_organization_billing(owner_id));

revoke all on function treq_internal.require_organization_manager(uuid, uuid)
  from public, anon, authenticated, service_role;
revoke all on function public.can_manage_organization_billing(uuid) from public, anon;
grant execute on function public.can_manage_organization_billing(uuid) to authenticated, service_role;
revoke all on function public.organization_set_role(uuid, uuid, uuid, text)
  from public, anon, authenticated;
grant execute on function public.organization_set_role(uuid, uuid, uuid, text) to service_role;
