-- Team projects: a project owned by a Team organization is shared with its
-- members. Personal projects stay local to the app.

create table public.organization_projects (
  id uuid primary key default gen_random_uuid(),
  org_id uuid not null references public.organizations (id) on delete cascade,
  name text not null check (char_length(name) between 1 and 100 and name = btrim(name)),
  repos jsonb not null default '[]' check (jsonb_typeof(repos) = 'array'),
  prompt text not null default '',
  context_paths jsonb not null default '[]' check (jsonb_typeof(context_paths) = 'array'),
  created_by uuid references auth.users (id) on delete set null default auth.uid(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create index idx_organization_projects_org on public.organization_projects (org_id);

alter table public.organization_projects enable row level security;
revoke all on public.organization_projects from anon;
grant select, insert, update, delete on public.organization_projects to authenticated;

create policy organization_projects_member_all on public.organization_projects
  for all to authenticated
  using (public.is_organization_member(org_id))
  with check (public.is_organization_member(org_id));
