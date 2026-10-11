-- Owners can promote members and demote other owners, an owner can delete
-- the organization, and deleting an account never leaves an organization
-- without an owner. Before this, a sole owner could neither leave nor hand
-- the organization over.
--
-- Transferring ownership is a promote followed by a leave. Like
-- 028_organizations_team.sql, every write is a security definer function
-- that only the service role can call, through the organizations Edge
-- Function. Refusals raise SQLSTATE PTnnn (HTTP nnn) with a stable code in
-- the hint.
--
-- Writes to an organization's installation follow its linked_user_id (the
-- policies of 003_merge_queue.sql), and 028 keeps that user an owner: a
-- member who leaves hands their installations to an owner. Demotion and
-- account deletion do the same here.

-- Returns 'promoted', or 'already_owner' when there was nothing to do.
create function public.organization_promote_member(p_actor uuid, p_org_id uuid, p_user_id uuid)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_role text;
begin
  perform 1 from public.organizations o where o.id = p_org_id for update;
  perform treq_internal.require_organization_owner(p_org_id, p_actor);

  v_role := treq_internal.organization_role(p_org_id, p_user_id);
  if v_role is null then
    raise exception 'That account is not a member of this organization.'
      using errcode = 'PT404', hint = 'member_not_found';
  end if;
  if v_role = 'owner' then
    return 'already_owner';
  end if;

  update public.organization_members m
  set role = 'owner'
  where m.org_id = p_org_id and m.user_id = p_user_id;
  return 'promoted';
end;
$$;

-- An owner demotes an owner to member: another owner, or themselves while
-- another owner remains. Returns 'demoted', or 'already_member'.
create function public.organization_demote_owner(p_actor uuid, p_org_id uuid, p_user_id uuid)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_role text;
  v_successor uuid;
begin
  -- Lock first, so two owners demoting each other cannot both succeed.
  perform 1 from public.organizations o where o.id = p_org_id for update;
  perform treq_internal.require_organization_owner(p_org_id, p_actor);

  v_role := treq_internal.organization_role(p_org_id, p_user_id);
  if v_role is null then
    raise exception 'That account is not a member of this organization.'
      using errcode = 'PT404', hint = 'member_not_found';
  end if;
  if v_role = 'member' then
    return 'already_member';
  end if;
  if (
    select count(*) from public.organization_members m
    where m.org_id = p_org_id and m.role = 'owner'
  ) <= 1 then
    raise exception 'An organization needs at least one owner. Make another member an owner first.'
      using errcode = 'PT409', hint = 'last_owner';
  end if;

  update public.organization_members m
  set role = 'member'
  where m.org_id = p_org_id and m.user_id = p_user_id;

  if p_actor <> p_user_id then
    v_successor := p_actor;
  else
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

  return 'demoted';
end;
$$;

-- An owner deletes the organization: its invites, its memberships and the
-- organization itself. Its installations become the deleting owner's
-- personal ones. Refused while a Team subscription is trialing, active
-- (including one set to cancel at the period end) or past_due, so nobody
-- is billed for an organization that no longer exists.
create function public.organization_delete(p_actor uuid, p_org_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform 1 from public.organizations o where o.id = p_org_id for update;
  perform treq_internal.require_organization_owner(p_org_id, p_actor);

  if treq_internal.organization_has_team(p_org_id) then
    raise exception 'This organization has a Team subscription. Cancel it with Manage billing, then delete the organization once the subscription has ended.'
      using errcode = 'PT409', hint = 'team_subscription_active';
  end if;

  update public.github_app_installations i
  set organization_id = null, linked_user_id = p_actor, updated_at = now()
  where i.organization_id = p_org_id;

  delete from public.organization_invites v where v.org_id = p_org_id;
  delete from public.organization_members m where m.org_id = p_org_id;
  delete from public.organizations o where o.id = p_org_id;
end;
$$;

revoke all on function public.organization_promote_member(uuid, uuid, uuid)
  from public, anon, authenticated;
revoke all on function public.organization_demote_owner(uuid, uuid, uuid)
  from public, anon, authenticated;
revoke all on function public.organization_delete(uuid, uuid)
  from public, anon, authenticated;

grant execute on function public.organization_promote_member(uuid, uuid, uuid) to service_role;
grant execute on function public.organization_demote_owner(uuid, uuid, uuid) to service_role;
grant execute on function public.organization_delete(uuid, uuid) to service_role;

-- ── Account deletion ──────────────────────────────────────────────────────
--
-- Runs before an auth.users row is deleted, for each organization the user
-- belongs to:
-- - another owner remains: nothing changes but the user's membership;
-- - no owner remains but members do: the longest-standing member becomes
--   the owner;
-- - nobody remains: the organization and its invites are deleted, and its
--   installations become personal. Their linked_user_id was this user (the
--   foreign key clears it) or nobody.
-- Installations of the organization that the user linked pass to the
-- remaining owner. The user's memberships are removed here rather than by
-- the foreign key's cascade, which runs at the end of the statement: when
-- one statement deletes several members, each row then sees the ones
-- before it already gone.
--
-- Deleting the organization does not cancel a Team subscription in Stripe.

create function treq_internal.organizations_on_account_delete()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_org_id uuid;
  v_successor uuid;
begin
  for v_org_id in
    select m.org_id from public.organization_members m
    where m.user_id = old.id
    order by m.org_id
  loop
    perform 1 from public.organizations o where o.id = v_org_id for update;

    delete from public.organization_members m
    where m.org_id = v_org_id and m.user_id = old.id;

    select m.user_id into v_successor
    from public.organization_members m
    where m.org_id = v_org_id
    order by (m.role = 'owner') desc, m.created_at, m.user_id
    limit 1;

    if v_successor is null then
      update public.github_app_installations i
      set organization_id = null, updated_at = now()
      where i.organization_id = v_org_id;
      delete from public.organization_invites v where v.org_id = v_org_id;
      delete from public.organizations o where o.id = v_org_id;
    else
      update public.organization_members m
      set role = 'owner'
      where m.org_id = v_org_id and m.user_id = v_successor and m.role <> 'owner';

      update public.github_app_installations i
      set linked_user_id = v_successor, updated_at = now()
      where i.organization_id = v_org_id
        and i.linked_user_id = old.id;
    end if;
  end loop;
  return old;
end;
$$;

revoke all on function treq_internal.organizations_on_account_delete()
  from public, anon, authenticated, service_role;

create trigger on_auth_user_deleted_organizations
  before delete on auth.users
  for each row execute function treq_internal.organizations_on_account_delete();
