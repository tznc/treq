-- Cloudflare Sandboxes as a provider (cloudflare-adapter.ts). BYOS only for
-- now: its machine types are not selectable, so managed defaults never pick
-- it, while the BYOS catalog lists every machine type of a saved key's kind.
-- Cloudflare's largest instance is 4 vCPU, so only s and m exist.

alter table public.sandbox_providers drop constraint sandbox_providers_kind_check;
alter table public.sandbox_providers add constraint sandbox_providers_kind_check
  check (kind in ('boat', 'local', 'daytona', 'fly_machines', 'e2b', 'cloudflare'));

alter table public.remote_instances drop constraint remote_instances_provider_kind_check;
alter table public.remote_instances add constraint remote_instances_provider_kind_check
  check (provider_kind in ('boat', 'local', 'daytona', 'fly_machines', 'e2b', 'cloudflare'));

insert into public.sandbox_providers (kind, display_name, billing_granularity_sec, capabilities) values
  ('cloudflare', 'Cloudflare', 1,
   '{"stopKeepsDisk":false,"execApi":true,"serviceSupervisor":false,"tcpProxy":true}');

-- vendor_type is the tier id; the bridge Worker maps s to standard-3
-- (2 vCPU / 8 GiB / 16 GB) and m to standard-4 (4 vCPU / 12 GiB / 20 GB).
-- Cloudflare picks the location, so every region is listed.
insert into public.provider_machine_types
  (id, provider_kind, vendor_type, tier_id, vcpu, ram_gib, disk_gb, regions, selectable)
values
  ('cloudflare:s', 'cloudflare', 's', 's', 2, 8, 16, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('cloudflare:m', 'cloudflare', 'm', 'm', 4, 12, 20, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false);

-- List price at full CPU (developers.cloudflare.com/containers/pricing,
-- 2026-11-05): $0.00002/vCPU-s + $0.0000025/GiB-s + $0.00000007/GB-s.
-- A sleeping container keeps no disk, so stopped storage is free.
insert into public.machine_type_prices
  (machine_type_id, running_nanousd_per_sec, stopped_nanousd_per_gb_sec)
values
  ('cloudflare:s', 61120, 0),
  ('cloudflare:m', 111400, 0);
