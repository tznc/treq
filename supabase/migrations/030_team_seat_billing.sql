-- Team has no member cap (prds/billing-and-teams.md, "Organizations and
-- seats"). Its flat price covers 5 members and each member beyond that is
-- billed as an extra seat: the organizations Edge Function sets the Stripe
-- subscription's quantity to the member count after every join, leave and
-- removal. Pending invites are not billed, so inviting no longer counts
-- seats. These replace the functions of 028_organizations_team.sql with the
-- seat checks removed.

-- Returns the invite and its token. The token is never stored, so this is
-- the only time anyone sees it. Inviting an address that already has a
-- pending invite issues a new token for that invite (the old link stops
-- working) and a fresh 7 days, instead of a second invite.
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
-- twice.
create or replace function public.organization_accept_invite(p_actor uuid, p_token text)
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
