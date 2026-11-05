// Preflight for a provider switch (docs/design/sandbox-provider-adapter.md
// §6). A switch starts the sandbox fresh, so before it runs we ask the
// source machine, per repo in `remote_repositories`, what would be lost:
// dirty working copies, commits not on any remote, and running agents.
//
// The script prints one tab-separated record per line:
//   REPO <path>, then MISSING, or VCS <jj|git|none>, DIRTY <n>,
//   UNPUSHED <n>, AGENTS <n>, ERROR <message>, and the work a re-clone drops
//   that version control does not see: IGNORED <n> with up to ten
//   IGNORED_PATH <path> lines (gitignored files such as .env, less
//   regenerable build output), STASHES <n>, and FILES <n> for a folder with
//   no version control.
// The parser is strict: anything it cannot read counts as possible lost
// work, so the user is asked rather than surprised.

const AGENT_PROCESS_NAMES =
  "claude|codex|gemini|opencode|aider|amp|cursor-agent";

// Gitignored paths a re-clone rebuilds on its own, so losing them is not lost work.
const REGENERABLE =
  "node_modules|target|dist|build|.venv|venv|__pycache__|.next|.turbo|.cache|.treq|.jj";

const PREFLIGHT_SCRIPT = `
set -u
lost_to_reclone() {
  i=$(git -C "$1" ls-files --others --ignored --exclude-standard --directory 2>/dev/null \\
    | grep -Ev '^(${REGENERABLE.replaceAll(".", "\\.")})(/|$)' || true)
  printf 'IGNORED\\t%s\\n' "$(printf '%s' "$i" | grep -c . || true)"
  [ -n "$i" ] && printf '%s\\n' "$i" | head -n 10 | while IFS= read -r f; do printf 'IGNORED_PATH\\t%s\\n' "$f"; done
  printf 'STASHES\\t%s\\n' "$(git -C "$1" stash list 2>/dev/null | grep -c . || true)"
}
for p in "$@"; do
  printf 'REPO\\t%s\\n' "$p"
  if [ ! -d "$p" ]; then printf 'MISSING\\n'; continue; fi
  if [ -d "$p/.jj" ] && command -v jj >/dev/null 2>&1; then
    printf 'VCS\\tjj\\n'
    if ! d=$(cd "$p" && jj log --no-graph -r 'working_copies() ~ empty()' -T 'change_id.short() ++ "\\n"' 2>&1); then
      printf 'ERROR\\t%s\\n' "$(printf '%s' "$d" | head -n1)"; continue
    fi
    u=$(cd "$p" && jj log --no-graph --ignore-working-copy \\
      -r '(::visible_heads() ~ ::remote_bookmarks()) ~ empty() ~ working_copies()' \\
      -T 'commit_id.short() ++ "\\n"' 2>/dev/null) || { printf 'ERROR\\tjj log failed\\n'; continue; }
    if [ -e "$p/.git" ]; then lost_to_reclone "$p"; fi
  elif [ -e "$p/.git" ]; then
    printf 'VCS\\tgit\\n'
    d=$(git -C "$p" status --porcelain 2>/dev/null) || { printf 'ERROR\\tgit status failed\\n'; continue; }
    u=$(git -C "$p" rev-list --branches --not --remotes 2>/dev/null) || { printf 'ERROR\\tgit rev-list failed\\n'; continue; }
    lost_to_reclone "$p"
  else
    printf 'VCS\\tnone\\n'; d=""; u=""
    printf 'FILES\\t%s\\n' "$(find "$p" -type f 2>/dev/null | head -n 1000 | grep -c . || true)"
  fi
  printf 'DIRTY\\t%s\\n' "$(printf '%s' "$d" | grep -c . || true)"
  printf 'UNPUSHED\\t%s\\n' "$(printf '%s' "$u" | grep -c . || true)"
  a=0
  for proc in /proc/[0-9]*; do
    comm=$(cat "$proc/comm" 2>/dev/null) || continue
    case "$comm" in ${AGENT_PROCESS_NAMES}) ;; *) continue ;; esac
    cwd=$(readlink "$proc/cwd" 2>/dev/null) || continue
    case "$cwd" in "$p"|"$p"/*) a=$((a + 1)) ;; esac
  done
  printf 'AGENTS\\t%s\\n' "$a"
done
`;

// Clones one repo onto a fresh machine. An existing checkout is kept, so a
// retried step is a no-op.
const RECLONE_SCRIPT = `
set -eu
url="$1"; path="$2"
if [ -e "$path/.jj" ] || [ -e "$path/.git" ]; then exit 0; fi
mkdir -p "$(dirname "$path")"
if command -v jj >/dev/null 2>&1; then
  jj git clone --colocate "$url" "$path"
else
  git clone "$url" "$path"
fi
`;

export function preflightCommand(repoPaths: readonly string[]): string[] {
  return ["bash", "-c", PREFLIGHT_SCRIPT, "preflight", ...repoPaths];
}

export function recloneCommand(
  remoteUrl: string,
  remotePath: string,
): string[] {
  return ["bash", "-c", RECLONE_SCRIPT, "reclone", remoteUrl, remotePath];
}

export interface RepoPreflight {
  path: string;
  vcs: "jj" | "git" | "none" | "unknown";
  dirty: number;
  unpushed: number;
  agents: number;
  /** Gitignored files (less regenerable build output), and up to ten of their paths. */
  ignored: number;
  ignored_paths: string[];
  stashes: number;
  /** Files in a folder with no version control. */
  files: number;
  error: string | null;
}

export interface PreflightReport {
  repos: RepoPreflight[];
  // True when switching could lose something the user has not pushed.
  requires_ack: boolean;
}

function count(value: string): number | null {
  return /^\d+$/.test(value) ? Number(value) : null;
}

// Counts an older script does not print read as zero.
function optionalCount(value: string | undefined): number | null {
  return value === undefined ? 0 : count(value);
}

const EMPTY = {
  dirty: 0,
  unpushed: 0,
  agents: 0,
  ignored: 0,
  ignored_paths: [] as string[],
  stashes: 0,
  files: 0,
};

export function parsePreflightOutput(
  stdout: string,
  repoPaths: readonly string[],
): PreflightReport {
  const records = new Map<string, Map<string, string>>();
  const ignoredPaths = new Map<string, string[]>();
  let current: Map<string, string> | null = null;
  let currentPath = "";
  for (const line of stdout.split("\n")) {
    if (line.length === 0) continue;
    const tab = line.indexOf("\t");
    const key = tab < 0 ? line : line.slice(0, tab);
    const value = tab < 0 ? "" : line.slice(tab + 1);
    if (key === "REPO") {
      current = new Map();
      currentPath = value;
      records.set(value, current);
      ignoredPaths.set(value, []);
    } else if (current && key === "IGNORED_PATH") {
      ignoredPaths.get(currentPath)!.push(value);
    } else if (current && !current.has(key)) {
      current.set(key, value);
    }
  }

  let requiresAck = false;
  const repos = repoPaths.map((path): RepoPreflight => {
    const rec = records.get(path);
    if (!rec) {
      requiresAck = true;
      return {
        path,
        vcs: "unknown",
        ...EMPTY,
        ignored_paths: [],
        error: "no preflight output",
      };
    }
    if (rec.has("MISSING")) {
      return {
        path,
        vcs: "none",
        ...EMPTY,
        ignored_paths: [],
        error: "missing",
      };
    }
    const vcsRaw = rec.get("VCS");
    const vcs = vcsRaw === "jj" || vcsRaw === "git" || vcsRaw === "none"
      ? vcsRaw
      : "unknown";
    const dirty = count(rec.get("DIRTY") ?? "");
    const unpushed = count(rec.get("UNPUSHED") ?? "");
    const agents = count(rec.get("AGENTS") ?? "");
    const ignored = optionalCount(rec.get("IGNORED"));
    const stashes = optionalCount(rec.get("STASHES"));
    const files = optionalCount(rec.get("FILES"));
    const counts = [dirty, unpushed, agents, ignored, stashes, files];
    let error = rec.get("ERROR") ?? null;
    if (!error && counts.includes(null)) error = "unreadable preflight output";
    if (error || counts.some((n) => (n ?? 0) > 0)) requiresAck = true;
    return {
      path,
      vcs,
      dirty: dirty ?? 0,
      unpushed: unpushed ?? 0,
      agents: agents ?? 0,
      ignored: ignored ?? 0,
      ignored_paths: ignoredPaths.get(path) ?? [],
      stashes: stashes ?? 0,
      files: files ?? 0,
      error,
    };
  });
  return { repos, requires_ack: requiresAck };
}
