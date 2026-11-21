# Treq Enhancements for Claude

A Chrome (Manifest V3) extension for Claude Code on the web (`claude.ai/code`).

- Names the browser tab after the open session (`<session title> · Claude Code`),
  read from the session's link in the sidebar.
- Adds **Treq** to a session's **Open in** menu and **Open in Treq** to a pull
  request's menu in the Changes panel. Both open
  `treq://claude/session?id=<session>[&repo=owner/repo&pr=N]`: Treq runs the
  session as a terminal agent (`claude --teleport`) and asks which open pull
  requests to open workspaces for.

## Develop

```bash
npm install
npm test
npm run build
```

Load it from `chrome://extensions` → Developer mode → **Load unpacked** → this folder.
