-- Team projects from 076_organization_projects.sql: members of an
-- organization read and write its projects; outsiders see nothing.
begin;
select plan(13);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000053a1', 'proj-owner@example.com'),
  ('00000000-0000-0000-0000-0000000053a2', 'proj-member@example.com'),
  ('00000000-0000-0000-0000-0000000053a3', 'proj-outsider@example.com');
insert into public.organizations (id, name) values
  ('00000000-0000-0000-0000-0000000053b1', 'Proj Org');
insert into public.organization_members (org_id, user_id, role) values
  ('00000000-0000-0000-0000-0000000053b1', '00000000-0000-0000-0000-0000000053a1', 'owner'),
  ('00000000-0000-0000-0000-0000000053b1', '00000000-0000-0000-0000-0000000053a2', 'member');

select ok((select relrowsecurity from pg_class where oid = 'public.organization_projects'::regclass),
  'RLS enabled on organization_projects');

set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000053a2","role":"authenticated"}', true);
select lives_ok(
  $$ insert into public.organization_projects (id, org_id, name, repos)
     values ('00000000-0000-0000-0000-0000000053c1', '00000000-0000-0000-0000-0000000053b1', 'Web', '["acme/web"]') $$,
  'a member creates a team project');
select is((select count(*)::int from public.organization_projects where name = 'Web'), 1, 'a member lists team projects');
select lives_ok(
  $$ update public.organization_projects set name = 'Web app' where id = '00000000-0000-0000-0000-0000000053c1' $$,
  'a member renames a team project');

select lives_ok(
  $$ insert into public.organization_projects (org_id, name, repos)
     values ('00000000-0000-0000-0000-0000000053b1', 'Urls', '["https://github.com/acme/api.git", "git@github.com:acme/cli.git"]') $$,
  'repos accept clone URLs');
select throws_ok(
  $$ insert into public.organization_projects (org_id, name, repos)
     values ('00000000-0000-0000-0000-0000000053b1', 'Local', '["/Users/me/code/web"]') $$,
  '23514', null, 'repos reject local paths');
select throws_ok(
  $$ insert into public.organization_projects (org_id, name, repos)
     values ('00000000-0000-0000-0000-0000000053b1', 'Bad', '[1]') $$,
  '23514', null, 'repos reject non-strings');

select lives_ok(
  $$ select public.put_organization_project('00000000-0000-0000-0000-0000000053c2',
       '00000000-0000-0000-0000-0000000053b1', 'Synced', '["acme/api"]', 'Use pnpm', '[]') $$,
  'put_organization_project inserts');
select lives_ok(
  $$ select public.put_organization_project('00000000-0000-0000-0000-0000000053c2',
       '00000000-0000-0000-0000-0000000053b1', 'Synced 2', '["acme/api"]', '', '[]') $$,
  'put_organization_project updates by id');

select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000053a3","role":"authenticated"}', true);
select is((select count(*)::int from public.organization_projects), 0, 'an outsider sees no team projects');
select throws_ok(
  $$ insert into public.organization_projects (org_id, name, repos)
     values ('00000000-0000-0000-0000-0000000053b1', 'Evil', '[]') $$,
  '42501', null, 'an outsider cannot create a team project');
select throws_ok(
  $$ select public.put_organization_project(gen_random_uuid(),
       '00000000-0000-0000-0000-0000000053b1', 'Evil', '[]', '', '[]') $$,
  '42501', null, 'an outsider cannot put a team project');
reset role;

select is((select name from public.organization_projects where id = '00000000-0000-0000-0000-0000000053c1'),
  'Web app', 'outsider changes nothing');

select * from finish();
rollback;
