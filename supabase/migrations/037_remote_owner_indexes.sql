-- Index owner_user_id on the remote_* tables that lack one.
--
-- Each table is read through an `owner_user_id = auth.uid()` policy (021),
-- so without an index a signed-in read scans every user's rows.
-- remote_instances, remote_client_keys, remote_instance_operations and
-- remote_audit_events already have an index leading with owner_user_id.

create index if not exists remote_endpoints_owner_user_id_idx
  on public.remote_endpoints (owner_user_id);
create index if not exists remote_endpoint_host_keys_owner_user_id_idx
  on public.remote_endpoint_host_keys (owner_user_id);
create index if not exists remote_endpoint_authorized_keys_owner_user_id_idx
  on public.remote_endpoint_authorized_keys (owner_user_id);
create index if not exists remote_repositories_owner_user_id_idx
  on public.remote_repositories (owner_user_id);
