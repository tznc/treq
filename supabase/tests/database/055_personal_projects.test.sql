-- Personal projects from 078_personal_projects.sql: a project with no
-- organization belongs to one user, who alone reads and writes it.
begin;
select plan(7);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000055a1', 'personal-owner@example.com'),
  ('00000000-0000-0000-0000-0000000055a2', 'personal-other@example.com');

set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000055a1","role":"authenticated"}', true);
select lives_ok(
  $$ select public.put_organization_project('00000000-0000-0000-0000-0000000055c1',
       null, 'Side', '["me/side"]', '', '[]') $$,
  'a user puts a personal project with no organization');
select is((select owner_user_id::text from public.organization_projects
            where id = '00000000-0000-0000-0000-0000000055c1'),
  '00000000-0000-0000-0000-0000000055a1', 'it is owned by the caller');
select lives_ok(
  $$ select public.put_organization_project('00000000-0000-0000-0000-0000000055c1',
       null, 'Side 2', '["me/side"]', '', '[]') $$,
  'the owner updates it');
select is((select count(*)::int from public.organization_projects), 1, 'the owner lists it');

select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000055a2","role":"authenticated"}', true);
select is((select count(*)::int from public.organization_projects), 0, 'another user sees no personal project');
select throws_ok(
  $$ select public.put_organization_project('00000000-0000-0000-0000-0000000055c1',
       null, 'Stolen', '["me/side"]', '', '[]') $$,
  '42501', null, 'another user cannot overwrite it');
reset role;

select throws_ok(
  $$ insert into public.organization_projects (name, repos) values ('Orphan', '[]') $$,
  '23514', null, 'a project needs an organization or an owner');

select * from finish();
rollback;
