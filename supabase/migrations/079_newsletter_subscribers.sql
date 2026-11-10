-- Email sign-ups from treq.dev Learn and tools pages.
--
-- Visitors never touch the table: they call subscribe_newsletter(), which
-- validates the address and ignores duplicates without saying so, so the
-- function cannot be used to check whether someone is subscribed. Only
-- service_role reads the list. unsubscribe_token backs the one-click
-- unsubscribe link once an email sender exists.

create table public.newsletter_subscribers (
  id uuid primary key default gen_random_uuid(),
  email text not null unique check (email = lower(email) and length(email) <= 254),
  source_path text check (length(source_path) <= 512),
  consented_at timestamptz not null default now(),
  unsubscribed_at timestamptz,
  unsubscribe_token uuid not null unique default gen_random_uuid()
);

alter table public.newsletter_subscribers enable row level security;
revoke all on public.newsletter_subscribers from anon, authenticated;

create function public.subscribe_newsletter(p_email text, p_source_path text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_email text := lower(trim(p_email));
begin
  if v_email is null
     or length(v_email) > 254
     or v_email !~ '^[^@\s]+@[^@\s]+\.[^@\s]+$' then
    raise exception 'invalid email' using errcode = '22023';
  end if;
  insert into public.newsletter_subscribers (email, source_path)
  values (v_email, left(p_source_path, 512))
  on conflict (email) do nothing;
end;
$$;

revoke all on function public.subscribe_newsletter(text, text) from public;
grant execute on function public.subscribe_newsletter(text, text) to anon, authenticated;
