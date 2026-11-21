# Treq Enhancements for Claude

A Chrome (Manifest V3) extension for Claude Code on the web (`claude.ai/code`).

- Names the browser tab after the open session (`<session title> · Claude Code`),
  read from the session's link in the sidebar.

## Develop

```bash
npm install
npm test
npm run build
```

Load it from `chrome://extensions` → Developer mode → **Load unpacked** → this folder.
