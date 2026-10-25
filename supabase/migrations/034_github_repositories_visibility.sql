-- github_repositories had two permissive SELECT policies (own installations,
-- organization installations). Postgres ORs them into one filter it cannot turn
-- into a semi-join, so listing a user's repositories scanned the whole table.
-- One policy over the visible installation ids can use idx_gh_repos_installation.
-- Visibility is unchanged: installations linked to the user, plus installations
-- attached to an organization the user belongs to.

create index if not exists idx_gh_installations_linked_user
  on public.github_app_installations (linked_user_id);

create function public.visible_installation_ids()
returns bigint[]
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(array_agg(i.id), array[]::bigint[])
  from public.github_app_installations i
  where i.linked_user_id = (select auth.uid())
    or (i.organization_id is not null and public.is_organization_member(i.organization_id));
$$;

revoke all on function public.visible_installation_ids() from public, anon;
grant execute on function public.visible_installation_ids() to authenticated, service_role;

drop policy "Users can view repos of own installations" on public.github_repositories;
drop policy "Members can view their organization's repositories" on public.github_repositories;

create policy "Users can view repos of visible installations"
  on public.github_repositories for select
  to authenticated
  using (installation_id = any (((select public.visible_installation_ids()))::bigint[]));
