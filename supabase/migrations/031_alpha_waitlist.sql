-- Private alpha waitlist for managed cloud workspaces and SSH Remote
-- Development (prds/marketing.md, "Alpha waitlist").
--
-- A signed-in user joins from the dashboard after ticking an explicit
-- consent box. The row stores only what the PRD allows: the account ID, the
-- consent record (date and form version), and the page the user joined
-- from. It is kept apart from billing.
--
-- Leaving or following an email's unsubscribe link sets unsubscribed_at.
-- The row and the account stay, so rejoining is an update that clears
-- unsubscribed_at and records a new consent. The unsubscribe token is a
-- bearer credential for the public alpha-unsubscribe Edge Function, so
-- users can neither read nor change it; only the service role (the future
-- email sender and that function) sees it.

create table public.alpha_waitlist (
  user_id uuid primary key references auth.users (id) on delete cascade,
  consented_at timestamptz not null,
  form_version text not null check (char_length(form_version) <= 64),
  -- A site path such as /roadmap. Bounded because users write it directly.
  source_page text check (char_length(source_page) <= 256),
  unsubscribed_at timestamptz,
  unsubscribe_token uuid not null default gen_random_uuid() unique,
  created_at timestamptz not null default now()
);

alter table public.alpha_waitlist enable row level security;

create policy "Users can view own alpha waitlist row"
  on public.alpha_waitlist for select
  to authenticated
  using (user_id = auth.uid());

create policy "Users can join the alpha waitlist"
  on public.alpha_waitlist for insert
  to authenticated
  with check (user_id = auth.uid());

create policy "Users can update own alpha waitlist row"
  on public.alpha_waitlist for update
  to authenticated
  using (user_id = auth.uid())
  with check (user_id = auth.uid());

-- 008_public_grants.sql gives every new public table full DML to anon and
-- authenticated. Replace that with column grants: users never touch
-- unsubscribe_token, and consented_at and created_at are set by the
-- trigger below, not by the browser. No delete: leaving sets
-- unsubscribed_at, and the row goes when the account is deleted.
revoke all on table public.alpha_waitlist from anon, authenticated;

grant select (user_id, consented_at, form_version, source_page, unsubscribed_at, created_at)
  on public.alpha_waitlist to authenticated;
grant insert (user_id, form_version, source_page, unsubscribed_at)
  on public.alpha_waitlist to authenticated;
grant update (user_id, form_version, source_page, unsubscribed_at)
  on public.alpha_waitlist to authenticated;

-- Consent and unsubscribe times come from the database clock:
--   * a new row, or a row that goes from unsubscribed to subscribed (a
--     rejoin) or changes form_version, gets consented_at = now();
--   * unsubscribing stamps unsubscribed_at = now() once, and repeating it
--     keeps the first time, so the unsubscribe link is idempotent.
create function public.alpha_waitlist_stamp()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if tg_op = 'INSERT' then
    new.consented_at := now();
    new.created_at := now();
    if new.unsubscribed_at is not null then
      new.unsubscribed_at := now();
    end if;
    return new;
  end if;

  if new.unsubscribed_at is null
     and (old.unsubscribed_at is not null or new.form_version is distinct from old.form_version) then
    new.consented_at := now();
  end if;

  if new.unsubscribed_at is not null then
    new.unsubscribed_at := coalesce(old.unsubscribed_at, now());
  end if;

  return new;
end;
$$;

-- Only the trigger calls this.
revoke all on function public.alpha_waitlist_stamp() from public, anon, authenticated;

create trigger alpha_waitlist_stamp
  before insert or update on public.alpha_waitlist
  for each row execute function public.alpha_waitlist_stamp();
