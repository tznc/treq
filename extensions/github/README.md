# Treq Enhancements for GitHub

A Chrome (Manifest V3) extension that adds **Open in Claude / Codex / Treq**
links to GitHub repository and pull request sidebars, and a prompt on every
code line (file view and pull request diffs) that starts an agent about it.

- Claude: Claude Code on the web with the repository preselected.
- Codex: Codex on the web.
- Treq: `treq://repo/open` on a repository, `treq://pr/open` on a pull request.

Hover a code line, pick Claude, Codex or Treq, type a prompt and press Enter.
The prompt starts with the line (`path:line (owner/repo@ref)`) and opens in
Claude Code on the web, the Codex app (`codex://new`), or Treq
(`treq://agent/kickoff`, on the pull request's head branch or the viewed ref).

## Develop

```bash
npm install
npm test
npm run build
```

Load it from `chrome://extensions` → Developer mode → **Load unpacked** → this folder.
