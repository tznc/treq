-- Regression test for 021_remote_rls_hardening.sql. The remote
-- control-plane tables used to give signed-in users `for all` policies
-- that only checked owner_user_id, so a user could attach a host key to
-- someone else's endpoint (the trust function then served it as trusted),
-- clear revoked_at on a revoked client key, or point a repository at
-- another user's instance. Users now only read their own rows; Edge
-- Functions write as the service role, and a trigger keeps every foreign
-- key on the same owner even for the service role.
begin;
select plan(29);

-- Fixtures, created as postgres (bypasses RLS).
insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-00000000000a', 'rls-a@example.com'),
  ('00000000-0000-0000-0000-00000000000b', 'rls-b@example.com');

insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset) values
  ('10000000-0000-0000-0000-00000000000a', '00000000-0000-0000-0000-00000000000a', 'boat', 'us_east', 'small'),
  ('10000000-0000-0000-0000-00000000000b', '00000000-0000-0000-0000-00000000000b', 'boat', 'us_east', 'small');

insert into public.remote_endpoints (id, owner_user_id, instance_id, source, display_name, hostname, username) values
  ('20000000-0000-0000-0000-00000000000a', '00000000-0000-0000-0000-00000000000a',
   '10000000-0000-0000-0000-00000000000a', 'managed', 'A', 'a.example.com', 'treq'),
  ('20000000-0000-0000-0000-00000000000b', '00000000-0000-0000-0000-00000000000b',
   '10000000-0000-0000-0000-00000000000b', 'managed', 'B', 'b.example.com', 'treq');

insert into public.remote_endpoint_host_keys (owner_user_id, endpoint_id, algorithm, fingerprint_sha256) values
  ('00000000-0000-0000-0000-00000000000a', '20000000-0000-0000-0000-00000000000a', 'ssh-ed25519', 'SHA256:a-host');

insert into public.remote_client_keys (id, owner_user_id, public_key, fingerprint_sha256, algorithm, revoked_at) values
  ('30000000-0000-0000-0000-00000000000a', '00000000-0000-0000-0000-00000000000a',
   'ssh-ed25519 AAAA a', 'SHA256:a-client', 'ssh-ed25519', now()),
  ('30000000-0000-0000-0000-00000000000b', '00000000-0000-0000-0000-00000000000b',
   'ssh-ed25519 AAAA b', 'SHA256:b-client', 'ssh-ed25519', null);

insert into public.remote_repositories (owner_user_id, endpoint_id, remote_path, display_name) values
  ('00000000-0000-0000-0000-00000000000a', '20000000-0000-0000-0000-00000000000a', '/home/treq/a', 'a');

-- ── Privileges ────────────────────────────────────────────────────────────
select table_privs_are('public', 'remote_endpoints', 'authenticated', array['SELECT'],
  'authenticated can only select remote_endpoints');
select table_privs_are('public', 'remote_endpoint_host_keys', 'authenticated', array['SELECT'],
  'authenticated can only select remote_endpoint_host_keys');
select table_privs_are('public', 'remote_client_keys', 'authenticated', array['SELECT'],
  'authenticated can only select remote_client_keys');
select table_privs_are('public', 'remote_repositories', 'authenticated', array['SELECT'],
  'authenticated can only select remote_repositories');
select table_privs_are('public', 'remote_endpoint_authorized_keys', 'authenticated', array['SELECT'],
  'authenticated can only select remote_endpoint_authorized_keys');
select table_privs_are('public', 'remote_instances', 'authenticated', array['SELECT'],
  'authenticated can only select remote_instances');
select table_privs_are('public', 'remote_audit_retention_config', 'authenticated', array[]::text[],
  'authenticated has no privileges on remote_audit_retention_config');
select table_privs_are('public', 'remote_endpoint_host_keys', 'anon', array[]::text[],
  'anon has no privileges on remote_endpoint_host_keys');
select ok(has_table_privilege('service_role', 'public.remote_endpoint_host_keys', 'INSERT, UPDATE, DELETE'),
  'service_role keeps write access to remote_endpoint_host_keys');

-- ── As user B ─────────────────────────────────────────────────────────────
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-00000000000b","role":"authenticated"}', true);

select throws_ok(
  $$insert into public.remote_endpoint_host_keys (owner_user_id, endpoint_id, algorithm, fingerprint_sha256)
    values ('00000000-0000-0000-0000-00000000000b', '20000000-0000-0000-0000-00000000000a', 'ssh-ed25519', 'SHA256:evil')$$,
  '42501', null,
  'user B cannot add a host key to user A''s endpoint');

select throws_ok(
  $$insert into public.remote_endpoint_host_keys (owner_user_id, endpoint_id, algorithm, fingerprint_sha256)
    values ('00000000-0000-0000-0000-00000000000b', '20000000-0000-0000-0000-00000000000b', 'ssh-ed25519', 'SHA256:self')$$,
  '42501', null,
  'user B cannot add a host key to their own endpoint directly either');

select throws_ok(
  $$insert into public.remote_repositories (owner_user_id, instance_id, remote_path, display_name)
    values ('00000000-0000-0000-0000-00000000000b', '10000000-0000-0000-0000-00000000000a', '/x', 'x')$$,
  '42501', null,
  'user B cannot register a repository on user A''s instance');

select throws_ok(
  $$insert into public.remote_endpoints (owner_user_id, instance_id, source, display_name, hostname, username)
    values ('00000000-0000-0000-0000-00000000000b', '10000000-0000-0000-0000-00000000000a', 'managed', 'x', 'evil.example.com', 'x')$$,
  '42501', null,
  'user B cannot create an endpoint on user A''s instance');

select is_empty(
  $$select 1 from public.remote_endpoint_host_keys where endpoint_id = '20000000-0000-0000-0000-00000000000a'$$,
  'user B cannot read user A''s host keys');

-- ── As user A ─────────────────────────────────────────────────────────────
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-00000000000a","role":"authenticated"}', true);

select throws_ok(
  $$update public.remote_client_keys set revoked_at = null where id = '30000000-0000-0000-0000-00000000000a'$$,
  '42501', null,
  'user A cannot clear revoked_at on their own client key');

select throws_ok(
  $$delete from public.remote_client_keys where id = '30000000-0000-0000-0000-00000000000a'$$,
  '42501', null,
  'user A cannot delete their client key rows directly');

select throws_ok(
  $$update public.remote_endpoint_host_keys set trusted_at = now(), generation = 99, revoked_at = null
    where endpoint_id = '20000000-0000-0000-0000-00000000000a'$$,
  '42501', null,
  'user A cannot rewrite host key trust state');

select throws_ok(
  $$update public.remote_endpoints set hostname = 'evil.example.com'
    where id = '20000000-0000-0000-0000-00000000000a'$$,
  '42501', null,
  'user A cannot repoint their managed endpoint');

select throws_ok(
  $$insert into public.remote_endpoint_authorized_keys (owner_user_id, endpoint_id, client_key_id)
    values ('00000000-0000-0000-0000-00000000000a', '20000000-0000-0000-0000-00000000000a', '30000000-0000-0000-0000-00000000000a')$$,
  '42501', null,
  'user A cannot record an authorized key install directly');

select results_eq(
  $$select fingerprint_sha256 from public.remote_endpoint_host_keys$$,
  $$values ('SHA256:a-host'::text)$$,
  'user A reads exactly their own host keys');

select ok(
  (select revoked_at is not null from public.remote_client_keys where id = '30000000-0000-0000-0000-00000000000a'),
  'user A still sees their client key as revoked');

select results_eq(
  $$select remote_path from public.remote_repositories$$,
  $$values ('/home/treq/a'::text)$$,
  'user A reads exactly their own repositories');

select results_eq(
  $$select id from public.remote_endpoints$$,
  $$values ('20000000-0000-0000-0000-00000000000a'::uuid)$$,
  'user A reads exactly their own endpoints');

-- ── As the service role (Edge Functions) ──────────────────────────────────
-- Writes are allowed, but the same-owner trigger still rejects a row that
-- references another user's endpoint, instance, or client key.
reset role;
set local role service_role;

select throws_ok(
  $$insert into public.remote_endpoint_host_keys (owner_user_id, endpoint_id, algorithm, fingerprint_sha256)
    values ('00000000-0000-0000-0000-00000000000b', '20000000-0000-0000-0000-00000000000a', 'ssh-ed25519', 'SHA256:evil')$$,
  '42501', null,
  'service role cannot attach a host key owned by B to A''s endpoint');

select throws_ok(
  $$insert into public.remote_repositories (owner_user_id, instance_id, remote_path, display_name)
    values ('00000000-0000-0000-0000-00000000000b', '10000000-0000-0000-0000-00000000000a', '/x', 'x')$$,
  '42501', null,
  'service role cannot register B''s repository on A''s instance');

select throws_ok(
  $$insert into public.remote_endpoint_authorized_keys (owner_user_id, endpoint_id, client_key_id)
    values ('00000000-0000-0000-0000-00000000000a', '20000000-0000-0000-0000-00000000000a', '30000000-0000-0000-0000-00000000000b')$$,
  '42501', null,
  'service role cannot install B''s client key on A''s endpoint');

select throws_ok(
  $$update public.remote_instances set endpoint_id = '20000000-0000-0000-0000-00000000000b'
    where id = '10000000-0000-0000-0000-00000000000a'$$,
  '42501', null,
  'service role cannot point A''s instance at B''s endpoint');

select lives_ok(
  $$insert into public.remote_endpoint_host_keys (owner_user_id, endpoint_id, algorithm, fingerprint_sha256, generation)
    values ('00000000-0000-0000-0000-00000000000a', '20000000-0000-0000-0000-00000000000a', 'ssh-ed25519', 'SHA256:a-rotated', 1)$$,
  'service role can still record a host key on the owner''s own endpoint');

select lives_ok(
  $$update public.remote_client_keys set revoked_at = now() where id = '30000000-0000-0000-0000-00000000000b'$$,
  'service role can still revoke a client key');

select * from finish();
rollback;
