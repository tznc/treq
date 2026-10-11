# Marketing

## Status

Draft.

This document proposes the go-to-market contract for Treq. It defines the audiences, what marketing may claim, the acquisition channels, the growth loops, and the gates between phases. The owner approves each remaining item in [Open decisions](#open-decisions) before the plan is Active.

Approval of this strategy does not authorize spending money, sending outreach, or creating external accounts. Each of those needs its own approval.

The plan skips a user-validation phase. There is no interview program or positioning test before execution. Live acquisition and conversion data corrects the plan instead, reviewed weekly through the GTM digest.

The automation that runs this plan lives in [Ziinc/biz-tools](https://github.com/Ziinc/biz-tools). The `gtm` and `content` blocks for `treq` in its `config.yaml` must match this document.

## Approved decisions

Approved by the owner on 2026-09-27:

1. No cash budget. Marketing is organic only.
2. Four marketing hours per week.
3. Treq supports macOS, Windows, and Linux. Every page states all three platforms.
4. Collect emails for the private alpha, with explicit consent.

Approved by the owner on 2026-10-05:

5. Pro is the cloud bundle: the GitHub App with its merge queue, managed cloud workspaces, mobile access, and Linear OAuth. GitHub and Linear features that run through the local `gh` CLI or a personal API key stay free for every repository.
6. The GitHub App requires Pro or Team.
7. Managed cloud workspaces require Pro or Team.
8. Pro has a 14-day trial. Checkout requires a card.
9. A Team plan costs US$199 per month for 5 members, plus US$8 per month for each extra member. An organization owns the subscription and the GitHub App installation.
10. The private alpha covers managed cloud workspaces and SSH Remote Development. Joining the waitlist requires a Treq account.
11. Service usage is measured from Supabase accounts.
12. GitHub Discussions is the primary community channel. A Discord server is also a community channel, but it is deferred and not a priority.

## Summary

Treq is the open-source Stacking Agent Development Environment. Each coding agent gets its own workspace, branches stack, and Treq rebases dependent work when the base moves. The desktop app is free, including GitHub and Linear through local tools. Pro is a cloud subscription at US$15 per user per month with a 14-day trial. It adds the features that need a server: the GitHub App and its merge queue, managed cloud workspaces, mobile access, and Linear OAuth. Team covers 5 members for US$199 per month, and each extra member is US$8 per month.

Marketing has one job: bring engineers who run several coding agents on one repository to the download, and move the ones who want the cloud features onto Pro or Team. Organic search carries acquisition. Stack comments on pull requests show Treq to reviewers who do not use it yet. Roadmap milestones supply the launch moments. The alpha waitlist collects signed-in engineers who want cloud workspaces early. There is no cash budget, so every channel runs on founder time.

## Product truth

Marketing copy may only claim what the product does today. Check claims against this section and against the code before publishing.

Shipped means available in the latest published release on GitHub Releases. Merged to `main` is not enough, because users cannot download it. The latest published release is v0.2.0, from 2026-08-09.

### Shipped

- Workspaces for parallel work, each with its own branch, files, terminal, and review.
- Stacked workspaces with automatic rebase when the base moves, built on Jujutsu over a colocated Git repository. Users do not need to install or learn `jj`.
- Agent sessions for Claude, Codex, and Cursor.
- Review, commit, push, and local merge from the app.
- GitHub integration through the local `gh` CLI: create and view pull requests, CI status, and inline review threads.
- Desktop builds for macOS on Apple silicon and Intel, distributed through GitHub Releases.
- The desktop app sends no feature usage, crash reports, or performance data. Diffs, comments, and terminal metadata stay on the user's machine.

### Merged, ships in v0.3.0

- Windows and Linux builds.
- Linear integration: view issues, create workspaces from issues, and auto-kickoff on a label. A personal API key is free. OAuth is Pro.

### Not shipped

- Pro checkout, the trial, and the Team plan.
- The merge queue.
- Workspace Checks.
- SSH Remote Development and managed cloud workspaces. See [Remote Development](./remote-development.md).
- Mobile apps. See [Mobile](./mobile.md).
- Stack comments on pull requests.

### Site copy that conflicts with product truth

Fix these before any amplification. A visitor who finds a false claim stops trusting the rest of the page.

1. Every page lists Windows and Linux, and the pricing page lists the Linear integration. No published release includes either. Publishing v0.3.0 resolves both.
2. "Upgrade to Pro" on the pricing page leads to a disabled button on the dashboard. Checkout resolves it.
3. Download links point at the old `Ziinc/treq` repository and its full release list. Point them at the latest release in `treq-dev/treq`.

## Positioning

Proposed line: "Run many coding agents on one repository. Treq stacks their branches and rebases them for you."

Supporting claims, each backed by a demonstration on the site:

- Each agent works in isolation, so agents do not overwrite each other's files.
- Dependent work stacks, so you review small layers instead of one large diff.
- Rebases happen when the base moves, so a stack stays current without manual work.
- The app is open source and keeps code on your machine.

Treq competes with Conductor, Claude Squad, Emdash, Orca, Superset, and Graphite. The comparison pages under `/compare/` hold the current differences. Every competitor claim must match that competitor's current public documentation on the day it is published.

## Audiences

In priority order:

1. Engineers who run two or more coding agents on the same repository and lose time to conflicts, stale branches, or large diffs. This group feels the core problem and can adopt the free app alone.
2. Teams reviewing a high volume of AI-generated pull requests. This group needs a merge queue and a shared GitHub App installation, which gives it a reason to buy Team.
3. Users of stacked pull requests who want an open-source alternative to Graphite. This group already understands stacking and needs the fewest explanations.

These are hypotheses. The plan tests them with acquisition data, not interviews: the landing page and query mix that bring downloads and paid subscriptions show which audience responds.

## Offer and conversion

| Plan | Price | Who it serves |
| --- | --- | --- |
| Free | US$0 | Every developer. The full desktop app, plus GitHub through the local `gh` CLI and Linear through a personal API key, for any repository. |
| Pro | US$15 per user per month, 14-day trial with a card | Engineers who want the cloud features: the GitHub App and its merge queue, a managed cloud workspace, mobile access, and Linear OAuth. |
| Team | US$199 per month for 5 members, US$8 per extra member | Teams that share one GitHub App installation and merge queue. Every member gets Pro. |

The conversion path is: search, stack comment, or community post, then site page, then download from GitHub Releases, then a first stacked workspace, then account creation, then trial, then Pro. The triggers to start a trial are turning on the merge queue, creating a cloud workspace, and connecting Linear with OAuth. Each one runs on the server, so a local build of the open-source app cannot turn it on. The trigger to move to Team is a second member of the same GitHub organization who needs the App.

The Team plan and the trial are approved decisions. Any further price change, discount, or plan needs its own decision.

## Channels

| Priority | Channel | Assets | Automation |
| --- | --- | --- | --- |
| 1 | Organic search | Learn articles, comparison pages, free tools under `/tools/`, skills directory | biz-tools daily keyword report and weekly Draft content PRs |
| 2 | Stack comments | One comment on each stacked pull request Treq creates, linking to treq.dev | Posted by the app |
| 3 | GitHub | README, release notes, changelog, GitHub Discussions | Release notes are GitHub auto-generated |
| 4 | Alpha waitlist | Join from the dashboard after sign-in, alpha invitations, alpha update emails | Waitlist counts reported in the GTM digest |
| 5 | Developer communities | One post per launch moment, written for that community's rules | None. The owner posts by hand. |
| 6 | Integration directories | GitHub Marketplace listing, Linear integration listing | None |

Organic search leads because it compounds and costs founder time, not cash. The free tools and the skills directory attract searchers who are not yet looking for Treq. Each of those pages must link to the relevant Learn article and to the download.

Community posts are limited to launch moments. A post without new product substance spends reputation that the next launch needs. Questions and feedback go to GitHub Discussions, which the site and the app link to. A Discord server comes later, once Discussions has regular traffic, because live chat needs moderation time the four-hour budget does not cover.

## Growth loops

The funnel in [Offer and conversion](#offer-and-conversion) is one pass. These loops turn usage into new visitors without cash:

1. **Stack comments.** Treq posts one comment on each stacked pull request it creates. The comment lists the stack and links to treq.dev with UTM parameters. Every reviewer on that pull request sees it, including reviewers who do not use Treq. Security and Privacy discloses it.
2. **Team invites.** The GitHub App installation belongs to the organization, not to one user. When a second member needs the merge queue, the owner moves to Team and invites them. Each invite brings a new user into the app.
3. **Skills directory.** Each public skill page links to the download and opens the skill in Treq. Searchers who find a skill install the app to use it.

## Content engine

The biz-tools pipeline drafts content from Search Console data:

1. The daily report finds queries where treq.dev gets impressions but ranks poorly or has no page.
2. Every Monday, Draft content picks the top two queries not yet drafted. Informational queries become `/learn/how-to/` pages. Comparison queries become `/compare/` pages. Pricing and signup queries are not drafted automatically.
3. Claude writes the article prose in the Treq voice defined by `.claude/skills/explain-to-me/SKILL.md`, grounded in this repository.
4. biz-tools opens a draft pull request here with the pages and their manifest under `marketing/content/drafts/`.

The owner reviews each draft pull request before merge:

- Every product claim matches [Product truth](#product-truth).
- No claim appears in the avoid list in biz-tools `config.yaml`.
- The page is added to `web/sidebarsLearn.ts` or `web/sidebarsCompare.ts`.
- The site builds and every link resolves.

Close a draft that does not meet the bar. biz-tools records the query, so the pipeline does not draft it again.

## Launch moments

Each roadmap milestone gets one launch:

| Milestone | Roadmap target | Launch assets |
| --- | --- | --- |
| Windows and Linux builds | v0.3.0 | Changelog entry, community post |
| Pro checkout and Team plan | After v0.3.0 | Changelog entry, pricing page, email to the alpha waitlist |
| Workspace Checks | 2026 Q3 | Changelog entry, Learn article on quality gates for agent output, community post |
| SSH Remote Development and cloud workspaces | 2026 Q4 | Changelog entry, docs, Learn article, community post, alpha invitations |

A launch waits for the feature to ship and for the site copy to match it. A roadmap date never overrides that rule. Alpha waitlist members hear about each launch first.

## Alpha waitlist

The private alpha covers managed cloud workspaces and SSH Remote Development. Engineers join the waitlist from the dashboard after they sign in. Joining gives marketing an account, so every waitlist member also counts as a signup.

- The join step states the purpose, which is alpha invitations and alpha updates, and asks for explicit consent. The consent record stores the date and the form version.
- Emails go to the account email from sign-in.
- Each email includes a one-click unsubscribe. An unsubscribe stops every later send and keeps the account.
- Waitlist status is stored apart from billing. Store only the account ID, the consent record, and the page the user joined from. Do not add tracking pixels.
- The owner invites waitlist members in batches and approves each send by hand.

## Phases and exit gates

The phases match `gtm.phases` for `treq` in biz-tools `config.yaml`. The weekly GTM digest reports gate status and suggests the next phase when every gate is met. The owner changes the phase by hand.

| Phase | Exit gates |
| --- | --- |
| Foundation: measurement and content engine | Search Console, Bing, and GA4 collection enabled in biz-tools. Stripe collection enabled for subscriptions. Every site-copy conflict in [Product truth](#product-truth) fixed. Alpha waitlist live with consent and unsubscribe. Draft content producing reviewed pull requests weekly for 4 consecutive weeks. A baseline of weekly organic clicks and weekly download clicks recorded. |
| Acquisition: organic growth | Weekly organic clicks reach twice the foundation baseline. At least five Learn or comparison pages each bring 20 or more clicks per week. |
| Conversion: Free to paid | The first 10 paying Pro or Team subscriptions. Monthly churn measured for three months. |

The acquisition and conversion thresholds are working proposals, pending approval. Once the foundation baseline exists, the acquisition gates become metric gates in biz-tools config. Until then they stay manual.

## Measurement

The desktop app has no telemetry, and marketing must not add any. The funnel is measured at the site, at GitHub, and at the service.

| Step | Source | Metric |
| --- | --- | --- |
| Discover | Search Console, Bing Webmaster Tools | Clicks, impressions, and position by query and page |
| Visit | GA4 on treq.dev | Sessions and landing pages |
| Refer | GA4 | Sessions from UTM-tagged stack comment links |
| Download intent | GA4 | Outbound clicks to GitHub Releases |
| Download | GitHub Releases API | Asset download counts per release |
| Sign up | Supabase | New accounts per week |
| Join alpha | Supabase | Waitlist joins and unsubscribes per week |
| Connect | Supabase | Accounts and organizations with a GitHub App installation |
| Trial | Stripe | New trials and trials that convert to paid |
| Buy | Stripe | New Pro and Team subscriptions, net revenue, refunds |
| Retain | Stripe | Monthly churn by plan |

Do not add fingerprinting or cross-site tracking. Report sample sizes with every rate. Never divide one source's count by another source's count and call the result a conversion rate unless both count the same people.

## Automation requirements

biz-tools has adapters for Search Console, Bing, GA4, and Stripe, and a daily keyword report. Content drafting, the GTM digest, and metric gates sit on an unmerged branch. It still needs:

1. The GTM automation branch merged, so drafting, the digest, and gates run.
2. Working credentials and valid workflow files, so scheduled collection runs.
3. A GitHub Releases adapter that records asset download counts per release.
4. A Supabase adapter that reports weekly accounts, GitHub App installations, waitlist joins, and unsubscribes as aggregate counts. It must not export personal data.
5. A Stripe adapter that reads subscriptions, so the digest can report trials, Pro and Team counts, and churn.
6. A GA4 outbound-click event for the download link, so the digest can count download intent.
7. Metric gates in biz-tools config for the acquisition phase, once the baseline exists.

## Non-goals

- A user interview or positioning-test program before execution.
- Telemetry in the desktop app.
- Any cash spend, including paid advertising and sponsorships.
- Referral rewards, affiliate programs, or a custom marketing CMS.
- Publishing drafted content without owner review.
- Marketing features that are not shipped.

## Open decisions

| ID | Decision | Options | Recommendation |
| --- | --- | --- | --- |
| T06 | Acquisition and conversion thresholds | The working proposals in [Phases and exit gates](#phases-and-exit-gates), or other numbers. | Approve the proposals and revisit them at the foundation exit. |
| T08 | Roadmap targets | Workspace Checks targeted 2026 Q3, which has passed. Set new targets for it and for SSH Remote Development. | Set targets in release numbers. |

## Risks

| Risk | Signal | Response |
| --- | --- | --- |
| Content drafts contain wrong product claims | Review finds errors in more than one draft in a month | Tighten the avoid list and the product truth section. Lower the weekly draft count. |
| Traffic without downloads | Organic clicks grow while download clicks stay flat | Review landing pages for a clear next step. Check that the traffic matches the audiences. |
| Downloads without paid conversion | Accounts grow but subscriptions do not | Check whether trial users turn on the merge queue or create a cloud workspace. Revisit which features Pro includes. |
| Stack comments read as spam | Maintainers complain, or more repositories turn the comment off | Shorten the comment. If complaints continue, make it opt-in. |
| Cloud workspace cost outruns revenue | Monthly VM spend per paying account exceeds the Pro price | Lower the base allocation or price the cloud workspace as an add-on. |
| Competitor pages go stale | A competitor changes features or pricing | Re-check each comparison page once a quarter. |
| Marketing work crowds out product work | Marketing time exceeds four hours per week for three weeks | Cut community posts first, then draft volume. |
