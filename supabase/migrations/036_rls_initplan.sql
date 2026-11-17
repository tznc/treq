-- Evaluate auth.uid() once per query in RLS policies, not once per row.
--
-- A policy written as `owner_user_id = auth.uid()` calls the function for
-- every row it checks. Wrapped as `(select auth.uid())`, Postgres plans it as
-- an InitPlan and runs it once (Supabase advisor lint auth_rls_initplan).
-- The 025+ billing and organization policies already use the wrapped form;
-- these are the ones 001 and 015-021 left bare. `alter policy` keeps each
-- policy's command and roles; only the expressions change.

alter policy "Users can view own profile" on public.profiles
  using ((select auth.uid()) = id);
alter policy "Users can update own profile" on public.profiles
  using ((select auth.uid()) = id);

alter policy "Users can view own tokens" on public.desktop_auth_tokens
  using ((select auth.uid()) = user_id and used = false and expires_at > now());
alter policy "Users can create own tokens" on public.desktop_auth_tokens
  with check ((select auth.uid()) = user_id);

alter policy "Users can view own installations" on public.github_app_installations
  using (linked_user_id = (select auth.uid()));

alter policy "Users can view own managed instance" on public.remote_instances
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own endpoints" on public.remote_endpoints
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own endpoint host keys" on public.remote_endpoint_host_keys
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own client keys" on public.remote_client_keys
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own authorized key installs" on public.remote_endpoint_authorized_keys
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own remote repositories" on public.remote_repositories
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own instance operations" on public.remote_instance_operations
  using (owner_user_id = (select auth.uid()));
alter policy "Users can view own audit events" on public.remote_audit_events
  using (owner_user_id = (select auth.uid()));
