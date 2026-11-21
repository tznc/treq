# Treq Enhancements for GitHub

A Chrome (Manifest V3) extension that adds **Open in Claude / Codex / Treq**
links to GitHub repository and pull request sidebars, and a prompt on every
code line (file view and pull request diffs) that starts an agent about it.

- Claude: Claude Code on the web with the repository preselected.
- Codex: Codex on the web.
- Treq: `treq://repo/open` on a repository, `treq://pr/open` on a pull request.

Hover a code line and press the Treq icon: type a prompt and send it with
⌘/Ctrl + Enter. The prompt starts with the file, line numbers and code of
the line, or of the selected lines around it (at most 10, eliding the
middle), and starts an agent through `treq://agent/kickoff` on the pull
request's head branch or the viewed ref.

## Develop

```bash
npm install
npm test
npm run build
```

Load it from `chrome://extensions` → Developer mode → **Load unpacked** → this folder.
