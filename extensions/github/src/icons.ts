// The desktop app's own icons, so the prompt looks like its composer.
import claude from "../../../src-tauri/assets/gpui-icons/agent-claude.svg";
import codex from "../../../src-tauri/assets/gpui-icons/agent-codex.svg";
import send from "../../../src-tauri/assets/gpui-icons/send.svg";
import treq from "../../../assets/logo.imageset/logo-square.svg";

/** An SVG file without its XML prolog, to inline in HTML. */
const inline = (svg: string) => svg.slice(svg.indexOf("<svg"));

export const ICONS = {
  claude: inline(claude),
  codex: inline(codex),
  treq: inline(treq),
  send: inline(send),
};
