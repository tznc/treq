-- Usage of an account being deleted. usage_reports and usage_intervals
-- cascade away with the user, and the hourly build waits for an hour to end
-- (plus 5 minutes), so the last one to two hours were never sent to Stripe.
-- delete-account builds this owner's hours up to now, including the current
-- partial hour, and sends them before cancelling.

create function public.build_final_usage_reports(p_user_id uuid, p_until timestamptz)
returns int
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_count int;
  -- No hour rounding and no grace: the account is going away, and its
  -- machines were deleted (billing closed) before this runs.
  v_cutoff timestamptz := least(p_until, now());
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  insert into public.usage_reports
    (identifier, owner_user_id, hour, seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd)
  select b.owner_user_id::text || ':' || to_char(b.hour at time zone 'UTC', 'YYYY-MM-DD"T"HH24'),
         b.owner_user_id, b.hour,
         round(sum(b.seconds))::int,
         round(sum(b.cost))::bigint,
         round(sum(treq_internal.billed_nanousd(b.cost, b.markup_bps, b.billing_mode)))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then treq_internal.billed_nanousd(b.cost, b.markup_bps, b.billing_mode)
                        else 0 end))::bigint
    from (
      select u.owner_user_id, h.hour, p.markup_bps, u.billing_mode,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour)) as seconds,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as cost
        from public.usage_intervals u
        join public.machine_type_prices p on p.id = u.price_id
        cross join lateral generate_series(
          date_trunc('hour', u.started_at),
          least(coalesce(u.ended_at, v_cutoff), v_cutoff) - interval '1 microsecond',
          interval '1 hour') as h(hour)
       where u.started_at < v_cutoff
         and u.owner_user_id = p_user_id
    ) b
   where b.hour < v_cutoff and b.seconds > 0
   group by b.owner_user_id, b.hour
  on conflict (identifier) do nothing;
  get diagnostics v_count = row_count;
  return v_count;
end;
$$;

revoke all on function public.build_final_usage_reports(uuid, timestamptz) from public, anon, authenticated;
grant execute on function public.build_final_usage_reports(uuid, timestamptz) to service_role;
