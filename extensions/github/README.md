# Treq Enhancements for GitHub

A Chrome (Manifest V3) extension that adds **Open in Claude / Codex / Treq**
links to GitHub repository and pull request sidebars.

- Claude: Claude Code on the web with the repository preselected.
- Codex: Codex on the web.
- Treq: `treq://repo/open` on a repository, `treq://pr/open` on a pull request.

## Develop

```bash
npm install
npm test
npm run build
```

Load it from `chrome://extensions` → Developer mode → **Load unpacked** → this folder.
