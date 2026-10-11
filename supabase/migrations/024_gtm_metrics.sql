-- Aggregate service metrics for the GTM digest in biz-tools
-- (prds/marketing.md, "Measurement" and "Automation requirements" item 4).
-- The gtm-metrics Edge Function calls this as the service role and returns
-- the result unchanged, so only counts leave Supabase: no rows, emails, or
-- IDs.
--
-- The window is whole UTC days, from the start of p_from to the end of
-- p_to. "Created", "joined", and "unsubscribed" counts fall inside the
-- window. Totals and "active" count rows created before the window ends,
-- in their current state: a deleted account or an unlinked installation
-- drops out of past windows too, and a member who left and rejoined only
-- counts by their latest unsubscribe time.
--
-- The alpha waitlist counts are null until 031_alpha_waitlist.sql has run,
-- so this migration does not depend on it. PL/pgSQL plans each statement
-- when it first runs, so the waitlist query below is never planned while
-- the table is missing.

create function public.gtm_metrics(p_from date, p_to date)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  window_start timestamptz;
  window_end timestamptz;
  joined bigint;
  unsubscribed bigint;
  active bigint;
begin
  if p_from is null or p_to is null or p_from > p_to then
    raise exception 'gtm_metrics needs from <= to, got % and %', p_from, p_to
      using errcode = '22023';
  end if;

  window_start := p_from::timestamp at time zone 'UTC';
  window_end := (p_to + 1)::timestamp at time zone 'UTC';

  if to_regclass('public.alpha_waitlist') is not null then
    select
      count(*) filter (where w.created_at >= window_start and w.created_at < window_end),
      count(*) filter (where w.unsubscribed_at >= window_start and w.unsubscribed_at < window_end),
      count(*) filter (where w.created_at < window_end
                         and (w.unsubscribed_at is null or w.unsubscribed_at >= window_end))
    into joined, unsubscribed, active
    from public.alpha_waitlist w;
  end if;

  return jsonb_build_object(
    'from', p_from,
    'to', p_to,
    'accounts_created', (
      select count(*) from auth.users u
      where not u.is_anonymous and u.created_at >= window_start and u.created_at < window_end
    ),
    'accounts_total', (
      select count(*) from auth.users u
      where not u.is_anonymous and u.created_at < window_end
    ),
    'github_app_installations_linked', (
      select count(distinct i.linked_user_id) from public.github_app_installations i
      where i.created_at < window_end
    ),
    'installations_total', (
      select count(*) from public.github_app_installations i
      where i.created_at < window_end
    ),
    'alpha_waitlist_joined', joined,
    'alpha_waitlist_unsubscribed', unsubscribed,
    'alpha_waitlist_active', active
  );
end;
$$;

-- Service role only (see 012_reclose_service_only_function_grants.sql for
-- why 008's default grant has to be revoked here).
revoke all on function public.gtm_metrics(date, date) from public, anon, authenticated;
grant execute on function public.gtm_metrics(date, date) to service_role;
