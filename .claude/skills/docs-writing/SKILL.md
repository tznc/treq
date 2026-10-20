---
name: docs-writing
description: >-
  Write or revise Treq technical docs under web/docs/ and shipped-behavior
  pages (roadmap, security, settings copy). Owns accuracy against current
  code, shipped vs WIP, and cross-page consistency. Use first for web/docs/**
  work.
---

# docs-writing (technical docs for Treq)

## When to use

Use this skill every time you write or revise **technical product documentation** for Treq:

- Anything under `web/docs/**` (concepts, how-tos, tutorials, reference, security)
- Product status pages that claim what ships today (`web/src/pages/roadmap.mdx`, changelog feature framing when it asserts availability)
- Sidebar / index / guides copy that points readers at those features

Do **not** treat changelog PR titles alone as proof a feature is user-ready.

For Learn articles (`web/learn/**`), READMEs that are not product how-tos, and voice-only edits, prefer the `writing` and `explain-to-me` skills. When a Learn article documents a concrete Treq UI behavior, still apply the accuracy rules below.

## Skill stack

| Concern | Skill |
|---|---|
| Product-doc accuracy, shipped vs WIP, prerequisites | **This skill (`docs-writing`)** |
| Voice, Orwell, ASD-STE100, banlists, readability script | `explain-to-me` |
| Learn-site skeleton, DefinitionCards, interlinking audit | `writing` |
| Formatting baseline | `web/STYLE_GUIDE.md` |

Read `explain-to-me` before drafting prose. Run its readability checker before you call the doc done.

## Mandatory research before drafting

Never draft from memory or from a changelog bullet alone.

1. **Find the code path.** Locate the UI entry points and backend handlers (components, hooks, Tauri commands, native Node addon surface). Prefer reading those files over summarizing commit messages.
2. **Separate shipped from WIP.** Code in tree is not the same as a finished product. Feature flags, Pro gates, incomplete workers, and explicit maintainer notes count. If a human says a feature is WIP, treat that as authoritative over changelog wording.
3. **Map auth and prerequisites.** Document what the user must install or sign into before the UI works. Silent failures (missing `gh`, non-GitHub remotes) belong in the docs.
4. **Diff UI surfaces.** The same verb in two places can mean different implementations (example: header **Create PR** vs Review-tab **Commit and create PR** title derivation). Document each path accurately.
5. **Read sibling docs.** Update pages that would contradict the new text (security, settings, guides, tutorials, roadmap, indexes, sidebars).

## Shipped vs WIP rules

State product behavior in present tense, as if the feature is finished. Put incompleteness only in callouts.

| State | How to write |
|---|---|
| Shipped | Present tense. Name exact UI labels. Link how-tos. |
| WIP / preview | Keep body copy in present tense for the intended design. Put incompleteness in `:::note[Work in progress]` at the top of that section or page. Do not weave "planned / intended / when finished" through every paragraph. |
| Roadmap | **Do not rewrite published roadmap prose.** After a roadmap page is published, only add status callouts on each announced feature (`:::tip[Partial ship]` / `:::note[Work in progress]`). Leave the original milestone text intact. |

Admonition title form matches the site: `:::tip[Shipped in v0.2.0]` and `:::note[Work in progress]`, then a blank line, then body, then `:::`.

Move WIP sentences out of opening paragraphs into callouts. Opening prose should describe the product, not apologize for unfinished slices.

## Accuracy checklist (must pass)

Before finishing any technical doc change, verify each claim:

- [ ] **UI labels** match the app string exactly (`Create PR`, `View PR`, `Add to Queue`, `Manage GitHub`, …).
- [ ] **Prerequisites** are complete and not overstated. Free vs Pro auth paths stay distinct when the product has them (example: Free uses `gh`, Pro can use the GitHub App, and `gh` may still help with fetches when present).
- [ ] **Plan gates** apply only to the surfaces that are actually gated.
- [ ] **No silent over-promise**: displaying CI is not auto-merge. Quoting a GitHub thread does not post to GitHub. Local review comments stay local unless copied or sent to an agent.
- [ ] **Auth prerequisites are not repeated** in every later section after you have already stated them once.
- [ ] **Related pages** no longer contradict the new page (security, settings, guides, tutorials, roadmap, concept/how-to indexes, `web/sidebars.ts`).
- [ ] **WIP features** use WIP callouts and do not rewrite published roadmap body copy.
- [ ] **Readability**: `python3 .claude/skills/explain-to-me/scripts/readability.py --strict path/to/doc.mdx` is clean of hard fails and Tier 1 words.

## Page types under `web/docs`

Match existing `web/docs` pages. Do not invent a new skeleton.

| Type | Lives in | Job |
|---|---|---|
| Concept | `web/docs/concepts/` | What the system is, boundaries, tables of behavior. Keep minor DX details out. |
| How-to | `web/docs/how-to/` | Task steps a reader can execute. No recommendations or "prefer X" advice. |
| Tutorial | `web/docs/tutorials/` | Longer walkthrough of a workflow |
| Reference | `web/docs/reference/` | Commands, shortcuts, exhaustive lists |
| Security / overview | `web/docs/security-and-privacy.md`, `web/docs/intro.md` | Cross-cutting claims that other pages must not contradict |

Register new pages in `web/sidebars.ts` and the relevant `index.md` DocCardList or concept list.

Frontmatter pattern:

```markdown
---
sidebar_position: N
---

# Title

_One-line italicized description._

Opening paragraph, 2 to 3 sentences. No lists here.
```

Use H2 only (house style). Prefer tables for comparisons. Bold UI labels on first meaningful use in a section. Interlink sibling docs in prose at first mention. Concept pages may omit `## Next Steps` when an Availability table already orients the reader. How-tos keep a short `## Next Steps` list.

## Concept-page discipline

Concept pages explain the system. They are not a dump of every UI affordance.

- Cut low-value DX sections (example: sidebar icon color tables that only mirror GitHub).
- After prerequisites are stated once, do not keep naming the CLI or auth tool in every later section.
- Prefer one clear sentence over a multi-paragraph mechanism when the reader does not need the mechanism to act.
- For primary UI controls, include a cropped generated screenshot when it clarifies the control better than prose.
- Availability / plan matrices beat long plan prose when Free vs Pro differs by feature.

## How-to discipline

- Steps only. Do not add "prefer", "you should usually", or other recommendations.
- Condense management surfaces into short declarative lines when a bullet laundry list adds no action.
- Link enabling steps to an on-page header when requirements mention enabling a feature.

## Screenshots

When a review or the flow needs a UI crop:

1. Capture the running app.
2. Store docs assets under `web/static/img/docs/`. The README/hero shot is committed as `assets/screenshots/code.png` and the web build copies it into `web/static/img/`. Landing images are generated into `web/static/img/landing/` by `npm run landing` in `e2e/desktop`; never commit them.
3. Embed with `ThemeAwareImage` from `@site/src/components/ThemeAwareImage` and a short caption.
4. Crop tightly to the control. Do not ship a full-window dashboard when a button and menu suffice.

## Cross-page consistency patterns

1. **Analogy vs product.** Do not leave docs that only say "like a GitHub PR" once a real integration exists. Point to the concept/how-to pages.
2. **Local vs remote review data.** If the product both keeps local comments and reads remote threads, say both. Do not let an older "never publishes" note imply remote threads are absent.
3. **Security follows features.** Optional cloud or CLI integrations must appear on Security and Privacy when they change where traffic goes.
4. **Settings follow features.** New toggles and Integrations surfaces need a Settings how-to mention.
5. **CLI follows features.** If `treq st` (or similar) prints new fields, update the CLI reference.
6. **Roadmap status callouts only.** After publish, add callouts. Do not rewrite the announced milestone body.

## Revision pass

1. Apply the accuracy checklist above against the code, not against the draft.
2. Run `explain-to-me` readability with `--strict` on every touched prose file.
3. Click every new in-body and Next Steps link target (or confirm the file exists under `web/`).
4. Skim the Docusaurus preview or built page for raw admonition syntax (`:::note` leaking as text means the title form is wrong).
5. If a human review corrected tone, WIP placement, or roadmap handling, propagate that rule into this skill when it is durable.

## Relationship to `/writing`

`writing` still owns Learn-site article shape, DefinitionCards, and bulk interlinking audits. For `web/docs/**` and product-status pages, **start here**. Pull voice rules from `explain-to-me`. Use `writing` when you need the Learn skeleton or the interlinking audit skill.

## Anti-patterns

- Claiming a feature shipped because it appears in changelog feat bullets or has UI chrome
- Rewriting published roadmap milestone prose instead of adding status callouts
- Mixing WIP hedges through every paragraph instead of one callout
- Documenting one Create/View path and implying all entry points behave identically
- Gating free-tier day-to-day `gh` flows behind Pro in prose
- Repeating `gh` / App prerequisites in every section after the prerequisites table
- Putting recommendations in how-to guides
- Leaving roadmap, security, or guides on the old story after a concept page changes
- Skipping the readability script because "it's only docs"
