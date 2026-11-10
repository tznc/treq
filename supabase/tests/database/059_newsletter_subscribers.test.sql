-- Newsletter sign-ups (079_newsletter_subscribers.sql): visitors can only
-- call subscribe_newsletter(). Nobody but service_role can read the list.
begin;
select plan(11);

select table_privs_are('public', 'newsletter_subscribers', 'anon', array[]::text[],
  'anon cannot touch the subscribers table');
select table_privs_are('public', 'newsletter_subscribers', 'authenticated', array[]::text[],
  'authenticated cannot touch the subscribers table');
select ok((select relrowsecurity from pg_class where oid = 'public.newsletter_subscribers'::regclass),
  'RLS enabled on public.newsletter_subscribers');
select function_privs_are('public', 'subscribe_newsletter', array['text', 'text'], 'anon',
  array['EXECUTE'], 'anon can subscribe');

set local role anon;
select lives_ok($$ select public.subscribe_newsletter('  Reader@Example.com ', '/learn/stacked-prs') $$,
  'a visitor can subscribe');
select lives_ok($$ select public.subscribe_newsletter('reader@example.com', '/tools/x') $$,
  'subscribing twice does not fail or reveal the address exists');
select throws_ok($$ select public.subscribe_newsletter('not-an-email', '/learn') $$,
  '22023', 'invalid email', 'an invalid address is rejected');
select throws_ok($$ select public.subscribe_newsletter(repeat('a', 250) || '@example.com', '/learn') $$,
  '22023', 'invalid email', 'an overlong address is rejected');

reset role;
select is((select count(*)::int from public.newsletter_subscribers), 1,
  'duplicates collapse to one row');
select is((select email from public.newsletter_subscribers), 'reader@example.com',
  'the address is trimmed and lowercased');
select is((select source_path from public.newsletter_subscribers), '/learn/stacked-prs',
  'the first sign-up page is kept');

select * from finish();
rollback;
