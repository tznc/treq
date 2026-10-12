import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { prepareAgentAutoCommand } from "./prepareAgentAutoCommand";

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    writeAgentCliFiles: vi.fn(),
    cleanupAgentCliFiles: vi.fn(),
    readFile: vi.fn(),
  };
});

const writtenFiles = {
  promptPath: "/tmp/treq-agent-prompt-1.txt",
  settingsPath: "/tmp/treq-agent-settings-1.json",
  skillDir: "/tmp/treq-agent-skills-1",
};

describe("prepareAgentAutoCommand", () => {
  beforeEach(() => {
    vi.mocked(api.readFile).mockRejectedValue(new Error("missing"));
    vi.mocked(api.writeAgentCliFiles).mockReset();
  });

  it("writes permission rules without a sandbox key", async () => {
    vi.mocked(api.writeAgentCliFiles).mockResolvedValueOnce(writtenFiles);

    await prepareAgentAutoCommand({
      agent: "claude",
      workspacePath: "/ws",
      repoPath: "/repo",
      sessionModel: null,
      treqBinDir: null,
    });

    const settingsJson = vi.mocked(api.writeAgentCliFiles).mock.calls[0]?.[1];
    expect(JSON.parse(settingsJson as string)).toEqual({
      permissions: {
        blockReadsOutsideWorkingDirectories: true,
        additionalDirectories: [],
        deny: [],
      },
    });
    expect(settingsJson).not.toContain("sandbox");
  });

  it("retries without cwd when writing project skills fails", async () => {
    vi.mocked(api.writeAgentCliFiles)
      .mockRejectedValueOnce(
        new Error(
          "Failed to create .agents/skills/treq: Read-only file system (os error 30)",
        ),
      )
      .mockResolvedValueOnce(writtenFiles);

    const { command, skillWriteWarning } = await prepareAgentAutoCommand({
      agent: "claude",
      workspacePath: "/ws",
      repoPath: "/repo",
      sessionModel: null,
      treqBinDir: null,
    });

    expect(api.writeAgentCliFiles).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.writeAgentCliFiles).mock.calls[0]?.[2]).toBe("/ws");
    expect(vi.mocked(api.writeAgentCliFiles).mock.calls[1]?.[2]).toBeNull();
    expect(command).toContain("--append-system-prompt-file");
    expect(skillWriteWarning).toMatch(/Could not write Treq skills/);
  });

  it("forwards a skill write warning from a successful write", async () => {
    vi.mocked(api.writeAgentCliFiles).mockResolvedValueOnce({
      ...writtenFiles,
      skillWriteWarning:
        "Failed to create .agents/skills/treq: Read-only file system",
    });

    const { skillWriteWarning } = await prepareAgentAutoCommand({
      agent: "claude",
      workspacePath: "/ws",
      repoPath: "/repo",
      sessionModel: null,
      treqBinDir: null,
    });

    expect(api.writeAgentCliFiles).toHaveBeenCalledTimes(1);
    expect(skillWriteWarning).toMatch(/Read-only file system/);
  });
});
