// The fake agent's paste mode (a `paste-agent` file in the fixture root):
// asks for bracketed paste, as agent TUIs do, and reads its prompt the way
// they do. Text between the paste markers is one draft; a bare Enter
// submits the draft. It prints each message it received, so a capture shows
// whether a multi-line message arrived whole or one line per submit.
process.stdin.setRawMode(true);
process.stdout.write("\x1b[?2004h\x1b[1m> Waiting for input\x1b[0m\r\n\r\n");
let draft = "";
let pasting = false;
let buf = "";
let count = 0;
process.stdin.on("data", (chunk) => {
  buf += chunk.toString("utf8");
  for (;;) {
    if (pasting) {
      const end = buf.indexOf("\x1b[201~");
      if (end < 0) return;
      draft += buf.slice(0, end);
      buf = buf.slice(end + 6);
      pasting = false;
      continue;
    }
    if (!buf) return;
    if (buf.startsWith("\x1b[200~")) {
      buf = buf.slice(6);
      pasting = true;
      continue;
    }
    if (buf.startsWith("\x1b") && buf.length < 6) return;
    const ch = buf[0];
    buf = buf.slice(1);
    if (ch === "\r") {
      count += 1;
      const lines = draft.split(/\r\n?|\n/);
      process.stdout.write(
        `\x1b[32m●\x1b[0m Message ${count} (${lines.length} lines):\r\n` +
          lines.map((l) => `  \x1b[2m│\x1b[0m ${l}`).join("\r\n") +
          "\r\n\r\n",
      );
      draft = "";
    } else if (ch === "\x03") {
      process.exit(0);
    } else {
      draft += ch;
    }
  }
});
