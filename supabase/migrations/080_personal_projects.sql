-- Personal projects: a project with no organization belongs to one user,
-- so it reaches that user's other devices (desktop and mobile). Only the
-- owner reads or writes it.

alter table public.organization_projects
  alter column org_id drop not null,
  add column owner_user_id uuid references auth.users (id) on delete cascade,
  add constraint organization_projects_one_owner
    check ((org_id is null) <> (owner_user_id is null));

create index idx_organization_projects_owner on public.organization_projects (owner_user_id);

create policy organization_projects_owner_all on public.organization_projects
  for all to authenticated
  using (owner_user_id = (select auth.uid()))
  with check (owner_user_id = (select auth.uid()));

-- p_org_id null puts a personal project owned by the caller.
create or replace function public.put_organization_project(
  p_id uuid, p_org_id uuid, p_name text, p_repos jsonb, p_prompt text, p_context_paths jsonb
)
returns void
language sql
security invoker
set search_path = ''
as $$
  insert into public.organization_projects (id, org_id, owner_user_id, name, repos, prompt, context_paths)
  values (p_id, p_org_id, case when p_org_id is null then (select auth.uid()) end,
          p_name, p_repos, p_prompt, p_context_paths)
  on conflict (id) do update
    set org_id = excluded.org_id, owner_user_id = excluded.owner_user_id, name = excluded.name,
        repos = excluded.repos, prompt = excluded.prompt,
        context_paths = excluded.context_paths, updated_at = now();
$$;
