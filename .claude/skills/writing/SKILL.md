---
name: writing
description: >-
  Write or revise Learn articles under web/learn/, READMEs, changelogs, and
  release notes: article structure and interlinking. For web/docs/** use
  docs-writing. Voice rules live in explain-to-me.
---

# Writing in the treq voice

## When to use

- Drafting or revising Learn articles under `web/learn/**`.
- Revising existing Learn prose to match the house voice.
- Running a de-slop edit pass on AI-generated Learn text before it ships.
- READMEs, changelogs, and release notes that are not product how-tos.

**Technical product docs (`web/docs/**`, roadmap feature status, security claims about product integrations) use `/docs-writing`.** That skill owns shipped-vs-WIP accuracy, prerequisites, and cross-page consistency. Come back here for Learn-site structure and interlinking.

**Voice and banlists live in `explain-to-me`.** Read
`.claude/skills/explain-to-me/SKILL.md` before you draft or revise. It owns:

- The house voice model
- Orwell's six rules and ASD-STE100
- Where software terms fit
- Voice rules, formatting hard bans
- Buzzword / marketing banlist and AI-tell banlist
- Annotated exemplars
- The readability checker script

This skill extends that foundation with site-doc structure and interlinking.
Where `web/STYLE_GUIDE.md` and `explain-to-me` agree on formatting, the style
guide wins. Where you need what the voice sounds like, use `explain-to-me`.

## Structure for concept and workflow articles

Follow the skeleton the existing docs use.

```markdown
---
sidebar_position: N
---

import DefinitionCard from '...'

# What is X?

_One-line italicized description, direct and precise._

Opening paragraph, 2 to 3 sentences. No lists here.

## Introduction
## Understanding the Concept
## Applying It in Practice
## Engineering Considerations
## Scaling & Operational Considerations
## Next Steps
```

- H2 only. Keep the structure flat, no H3 or H4.
- Introduce each key term with a `<DefinitionCard term=... definition=... />` and
  bold the term on first mention in prose.
- Use `<ThemeAwareImage src=... alt=... />` for diagrams.
- Use tables for side-by-side comparisons and dense reference data.
- Mark version-specific features with the admonition:
  `:::tip Added in v1.8.0` ... `:::`.
- `## Next Steps` is a short list of links to sibling docs.

Prose inside every section must follow `explain-to-me`.

## Answers: hub and spoke pages

`web/learn/answers/` holds short pages that each answer one exact search
query, such as an error message or "how do I X". They use a hub and spoke
model:

- **Hub:** `web/learn/answers/<topic>.md`. A short intro and `<DocCardList />`,
  which lists every spoke in the folder automatically. Hubs exist for
  `worktrees`, `rebasing-and-stacks` and `agents-and-git`. Add a hub only when
  a new topic has at least three spokes.
- **Spoke:** `web/learn/answers/<topic>/<query-slug>.md`. The sidebar picks it
  up automatically. Name the file after the query people type.

Every spoke uses this skeleton, 300 to 600 words:

```markdown
---
sidebar_position: N
description: The answer in one sentence, with the exact error text or query.
---

# Fix "<error>" | <Task phrased as the query>

_The answer in one or two sentences._

## The Fix
Commands first, copy-pasteable.

## Why It Happens
The mechanism, 1 to 2 short paragraphs. Quote the exact error message.

## How Treq Handles It
One paragraph, shipped behavior only, linked to the doc that covers it.

## Related
2 to 3 links: sibling spokes first, then the concept article.
```

Rules for spokes:

- Lead with the fix. A reader who stops after `## The Fix` should be done.
- Run every command and copy every error message from a real Git run. Name
  the Git version when output differs between versions.
- `## How Treq Handles It` states only what the docs under `web/docs/` say
  ships today. Leave the section out rather than stretch a claim.
- Link each spoke to at least one sibling spoke, so spokes link across, not
  only up to the hub.

## Interlinking across the content site

Treat every article as a node in the site, not a standalone page. Readers
land on one doc from search and decide whether to keep reading the site from
whether it points them somewhere else useful. Search engines read the same
signal: an article whose key terms link to the other articles that cover them
is what makes the site's internal link graph work for SEO. An article with no
outbound links and no inbound links is dead weight in that graph, regardless
of how good the prose is.

### Where links go

**Inline, in prose, at first meaningful mention.** When you name a concept,
tool, or workflow that has its own article elsewhere on the site, link it the
first time it appears in the body, not every time it recurs.

> Use [separate clones](./git-worktrees-vs-clones) when tasks need
> independent refs, remotes, or repository configuration.

This is a real sentence doing real work. The link is not bolted on. Do not
add a second link to the same target later in the same doc, and do not
interrupt a sentence's flow just to fit a link in: if no natural mention
exists, wait for the `## Next Steps` list instead of forcing one.

**`## Next Steps` (or `## Related`) at the end.** A short list, 3 to 5 items,
each a link plus a one-clause description of what the reader gets:

```markdown
## Next Steps

- [What are Stacked PRs?](./stacked-prs): manage branches that depend on one another
- [Stacked PR Workflow](/learn/workflows/git/stacked-pr): apply rebasing to a pull-request stack
```

This section is a map, not a repeat of the body. Do not just relist links
already made in prose, add the adjacent articles a reader would want next.

### Link syntax

- Same directory or a near sibling: relative, no extension:
  `[text](./git-worktrees-vs-clones)`.
- Anywhere else on the site: absolute from the content root, no extension:
  `[text](/learn/concepts/git/git-worktrees)`, `[text](/docs/concepts/workspaces)`.
- Never link a bare URL or write "click here". The anchor text is the term or
  phrase itself, exactly as it reads in the sentence.
- Verify the target actually exists (check the file under `web/learn/` or
  `web/docs/`, or run the audit-interlinking skill's checker) before writing
  the path. A guessed path that is close but wrong is a broken link.

### What earns a link

Link a term the first time you use it if, and only if, another article on the
site is the definitive treatment of that term. Do not link:

- A word that merely resembles another doc's title in a different sense.
- Your own doc's own subject (do not link a concept article to itself).
- A tangential mention that is not what the sentence is actually about.

Every new or revised article should end up with at least 2 to 3 in-body links
to genuinely relevant existing articles, plus a `## Next Steps` list. If you
cannot find that many relevant targets honestly, write fewer rather than
padding with weak matches. A forced link reads exactly like the AI tells
`explain-to-me` already bans: decoration instead of information.

### Auditing across the whole site

For a bulk check across many articles (broken links, orphaned pages, articles
that are under-linked, or finding where an article's key terms should link
out) use the `audit-interlinking` skill. It runs a link-graph checker and
turns the results into a worklist you still verify by reading, since keyword
matches are candidates, not verdicts.

## Revision and de-slop pass

Run this against any draft before you return it.

1. Apply the `explain-to-me` quick check (Orwell, ASD-STE100, hard bans,
   buzzwords, AI tells). Run the checker script:
   `python3 .claude/skills/explain-to-me/scripts/readability.py path/to/doc.mdx`
   Fix every hard ban, buzzword, and AI tell it reports, and pull long
   sentences apart.
2. Check paragraph length. Anything over 4 sentences gets split.
3. Check every claim is declarative, not hedged.
4. Remove any sentence that restates a point already made.
5. Verify against the three criteria from
   `web/learn/workflows/ai/ai-documentation-writing.md`: technical accuracy (every
   claim checks out against the source), reader appropriateness (the intended
   reader understands it at first read), and task completeness (a reader can act
   on it using only this text).
6. Check interlinking: at least 2 to 3 in-body links to genuinely relevant
   existing articles, a `## Next Steps` list, and every link target verified
   to exist. See the Interlinking section above.
