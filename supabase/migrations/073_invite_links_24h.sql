-- Invite links work for 24 hours instead of 7 days. A fresh link (the
-- dashboard's Generate invite link, or New link on a pending invite)
-- restarts the 24 hours. Same function as 069 otherwise.

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
      expires_at = now() + interval '24 hours'
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
      now() + interval '24 hours'
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
