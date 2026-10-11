import { describe, expect, it } from "vitest";
import {
  appendAgentPrompt,
  buildAgentAutoCommand,
  buildClaudeFilesystemSettings,
  buildTreqAgentSystemPrompt,
  claudeLocalSettingsPath,
  cursorPromptFileContents,
  mergeClaudeLocalSettings,
} from "./agentCommand";

describe("appendAgentPrompt", () => {
  it("separates a prompt beginning with hyphens from CLI options", () => {
    expect(
      appendAgentPrompt(
        "codex -c 'instructions=treq'",
        "---- test failure ----",
      ),
    ).toBe("codex -c 'instructions=treq' -- '---- test failure ----'");
  });

  it("quotes shell metacharacters in the prompt", () => {
    expect(appendAgentPrompt("codex", "it's $HOME `pwd`!")).toBe(
      "codex -- 'it'\\''s $HOME `pwd`!'",
    );
  });
});

describe("buildTreqAgentSystemPrompt", () => {
  it("describes a workspace and limits direct file access to it", () => {
    const prompt = buildTreqAgentSystemPrompt({
      repoPath: "/repos/treq",
      workspacePath: "/repos/treq/.treq/workspaces/fix-parser",
    });

    expect(prompt).toContain("You are operating inside a Treq workspace.");
    expect(prompt).toContain(
      "Your current working directory and direct filesystem scope is /repos/treq/.treq/workspaces/fix-parser.",
    );
    expect(prompt).toContain("The Treq home repository is /repos/treq.");
    expect(prompt).toContain(
      "The home repository and sibling workspaces are outside your direct filesystem scope.",
    );
    expect(prompt).toContain(
      "You may run treq CLI commands even when they create or manage workspaces outside the current working directory.",
    );
    expect(prompt).toContain(
      "A Treq skill named treq is loaded automatically for this session.",
    );
    expect(prompt).not.toMatch(/[\r\n]/);
    expect(prompt).not.toContain("\\n");
  });

  it("describes a home repository without workspace-only exclusions", () => {
    const prompt = buildTreqAgentSystemPrompt({
      repoPath: "/repos/treq",
      workspacePath: null,
    });

    expect(prompt).toContain("You are operating in the Treq home repository.");
    expect(prompt).toContain(
      "Your current working directory and direct filesystem scope is /repos/treq.",
    );
    expect(prompt).toContain(
      "You may freely read and write files within this directory and its descendants.",
    );
    expect(prompt).toContain(
      "Do not directly read, write, edit, or delete files outside this directory.",
    );
    expect(prompt).not.toContain("sibling workspaces");
    expect(prompt).not.toContain("outside your direct filesystem scope");
    expect(prompt).not.toMatch(/[\r\n]/);
  });
});

describe("buildTreqAgentSystemPrompt with supporting repositories", () => {
  it("lists supporting repositories as read-only with treq -r delegation", () => {
    const prompt = buildTreqAgentSystemPrompt({
      repoPath: "/repos/app",
      workspacePath: null,
      supportingRepoPaths: ["/repos/api", "/repos/web"],
    });

    expect(prompt).toContain(
      "Supporting repositories: /repos/api, /repos/web.",
    );
    expect(prompt).toContain(
      "You may read files in supporting repositories but must not write to them directly.",
    );
    expect(prompt).toContain("treq add -r <repo> <branch>");
    expect(prompt).toContain('treq agent -r <repo> <branch> "<prompt>"');
    expect(prompt).not.toMatch(/[\r\n]/);
  });

  it("omits supporting repository guidance when none are linked", () => {
    const prompt = buildTreqAgentSystemPrompt({
      repoPath: "/repos/app",
      workspacePath: null,
      supportingRepoPaths: [],
    });

    expect(prompt).not.toContain("Supporting repositories");
  });
});

describe("buildClaudeFilesystemSettings", () => {
  it("allows workspace reads and writes while denying the home repository", () => {
    expect(
      buildClaudeFilesystemSettings({
        repoPath: "/repos/treq",
        workspacePath: "/repos/treq/.treq/workspaces/fix-parser",
      }),
    ).toEqual({
      filesystem: {
        denyRead: ["/repos/treq"],
        allowRead: ["/repos/treq/.treq/workspaces/fix-parser"],
        allowWrite: ["/repos/treq/.treq/workspaces/fix-parser"],
      },
    });
  });

  it("allows home repository reads and writes without workspace exclusions", () => {
    expect(
      buildClaudeFilesystemSettings({
        repoPath: "/repos/treq",
        workspacePath: null,
      }),
    ).toEqual({
      filesystem: {
        allowRead: ["/repos/treq"],
        allowWrite: ["/repos/treq"],
      },
    });
  });
});

describe("buildClaudeFilesystemSettings with supporting repositories", () => {
  it("allows reading supporting repositories without writing them", () => {
    expect(
      buildClaudeFilesystemSettings({
        repoPath: "/repos/app",
        workspacePath: "/repos/app/.treq/workspaces/feat",
        supportingRepoPaths: ["/repos/api"],
      }),
    ).toEqual({
      filesystem: {
        denyRead: ["/repos/app"],
        allowRead: ["/repos/app/.treq/workspaces/feat", "/repos/api"],
        allowWrite: ["/repos/app/.treq/workspaces/feat"],
      },
    });
    expect(
      buildClaudeFilesystemSettings({
        repoPath: "/repos/app",
        workspacePath: null,
        supportingRepoPaths: ["/repos/api"],
      }),
    ).toEqual({
      filesystem: {
        allowRead: ["/repos/app", "/repos/api"],
        allowWrite: ["/repos/app"],
      },
    });
  });
});

describe("mergeClaudeLocalSettings", () => {
  it("overlays filesystem restrictions and drops sandbox keys", () => {
    const filesystem = buildClaudeFilesystemSettings({
      repoPath: "/repos/treq",
      workspacePath: "/repos/treq/.treq/workspaces/fix-parser",
    });
    expect(
      mergeClaudeLocalSettings(
        {
          extra: true,
          sandbox: { enabled: true, filesystem: { allowRead: ["/old"] } },
        },
        filesystem,
      ),
    ).toEqual({
      extra: true,
      filesystem: filesystem.filesystem,
    });
  });

  it("uses filesystem restrictions alone when no local file exists", () => {
    const filesystem = buildClaudeFilesystemSettings({
      repoPath: "/repos/treq",
      workspacePath: null,
    });
    expect(mergeClaudeLocalSettings(null, filesystem)).toEqual({
      filesystem: filesystem.filesystem,
    });
  });
});

describe("claudeLocalSettingsPath", () => {
  it("points at .claude/settings.local.json under the cwd", () => {
    expect(claudeLocalSettingsPath("/repos/treq/")).toBe(
      "/repos/treq/.claude/settings.local.json",
    );
  });
});

describe("cursorPromptFileContents", () => {
  it("combines system prompt with an optional pending prompt", () => {
    expect(cursorPromptFileContents("sys", "do the thing")).toBe(
      "sys do the thing",
    );
    expect(cursorPromptFileContents("sys")).toBe("sys");
  });
});

describe("buildAgentAutoCommand", () => {
  const files = {
    promptPath: "/tmp/treq-agent-prompt-1.txt",
    settingsPath: "/tmp/treq-agent-settings-1.json",
    skillDir: "/tmp/treq-agent-skills-1",
  };

  it("points Claude at filesystem settings and the prompt file", () => {
    const command = buildAgentAutoCommand({
      agent: "claude",
      files: { ...files, claudeSkillPath: "/ws/.claude/skills/treq" },
      pendingPrompt: "fix the bug",
    });

    expect(command).toContain("--settings '/tmp/treq-agent-settings-1.json'");
    expect(command).not.toContain("sandbox");
    expect(command).toContain(
      "--append-system-prompt-file '/tmp/treq-agent-prompt-1.txt'",
    );
    expect(command).not.toContain("--add-dir");
    expect(command).not.toContain("--append-system-prompt ");
    expect(command).not.toContain("You are operating");
    expect(command).toContain(
      `trap "rm -rf '/tmp/treq-agent-prompt-1.txt' '/tmp/treq-agent-skills-1' '/ws/.claude/skills/treq' '/tmp/treq-agent-settings-1.json'" EXIT`,
    );
    expect(command).toContain("-- 'fix the bug'");
  });

  it("loads Codex developer instructions from a file via cat", () => {
    const command = buildAgentAutoCommand({
      agent: "codex",
      files: {
        promptPath: files.promptPath,
        skillDir: files.skillDir,
        agentsSkillPath: "/ws/.agents/skills/treq",
      },
    });

    expect(command).toContain(
      `codex -c "developer_instructions=$(cat -- '/tmp/treq-agent-prompt-1.txt')"`,
    );
    expect(command).not.toContain("--append-system-prompt");
    expect(command).toContain(
      `trap "rm -rf '/tmp/treq-agent-prompt-1.txt' '/tmp/treq-agent-skills-1' '/ws/.agents/skills/treq'" EXIT`,
    );
  });

  it("loads the Cursor prompt from a file; skills come from .agents/skills", () => {
    const command = buildAgentAutoCommand({
      agent: "cursor",
      permissionMode: "plan",
      files: {
        promptPath: files.promptPath,
        skillDir: files.skillDir,
        agentsSkillPath: "/ws/.agents/skills/treq",
      },
    });

    expect(command).toContain("cursor-agent --plan -- ");
    expect(command).not.toContain("--plugin-dir");
    expect(command).toContain(`"$(cat -- '/tmp/treq-agent-prompt-1.txt')"`);
    expect(command).toContain(
      `trap "rm -rf '/tmp/treq-agent-prompt-1.txt' '/tmp/treq-agent-skills-1' '/ws/.agents/skills/treq'" EXIT`,
    );
  });

  it.each([
    "claude",
    "codex",
    "cursor",
    "copilot",
  ] as const)("tells Treq when the %s process exits on its own", (agent) => {
    const command = buildAgentAutoCommand({
      agent,
      treqBinDir: "/opt/treq/bin",
      files,
    });

    expect(
      command.startsWith("export PATH='/opt/treq/bin':$PATH; ( trap"),
    ).toBe(true);
    // Runs after the agent subshell, and not when Treq killed it (exit >= 128).
    expect(command).toMatch(
      /\); \[ \$\? -lt 128 \] && treq notify --agent-exited >\/dev\/null 2>&1$/,
    );
  });
});
