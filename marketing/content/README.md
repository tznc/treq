# Content manifests

This directory holds publication manifests for treq.dev content. The
[tznc/biz-tools](https://github.com/tznc/biz-tools) repository runs the
workflows that read and write them. See its `docs/gtm-automation.md` for the full loop.

- **Draft content** runs weekly. It turns the newest Search Console keyword gaps into
  pages under `web/learn/how-to/` or `web/compare/`, writes the manifest to
  `drafts/`, and opens a draft pull request here.
- **Publish project content** renders a manifest written by hand. Choose project
  `treq` and give a manifest path under `marketing/content/`.

Merging the pull request publishes the pages. Before you merge a draft:

1. Check each product claim against the code and docs.
2. Add the page to `web/sidebarsLearn.ts` or `web/sidebarsCompare.ts`.
3. Build the site to confirm links resolve.

A manifest is JSON:

```json
{
  "schemaVersion": "1",
  "publicationId": "treq-drafts-2026-09-28",
  "siteOrigin": "https://treq.dev",
  "entries": [
    {
      "type": "learn-how-to",
      "slug": "run-claude-code-in-parallel",
      "title": "Run Claude Code in Parallel",
      "description": "Run several Claude Code sessions in one repository, each in its own workspace.",
      "date": "2026-09-28",
      "canonical": "https://treq.dev/learn/how-to/run-claude-code-in-parallel",
      "body": "Markdown body. No HTML, JSX, or frontmatter."
    }
  ]
}
```

`type` is a content type defined for treq in biz-tools `config.yaml`, currently
`learn-how-to` or `compare`.
