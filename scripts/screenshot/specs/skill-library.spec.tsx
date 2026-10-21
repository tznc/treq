import * as React from "react";
import { expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import crypto from "crypto";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { uninstallSkill } from "../../../src/lib/api";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

function skillChecksum(files: { path: string; content: Buffer }[]): string {
  const hash = crypto.createHash("sha256");
  for (const file of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
    hash.update(file.path);
    hash.update(Buffer.from([0]));
    const len = Buffer.alloc(8);
    len.writeBigUInt64BE(BigInt(file.content.length));
    hash.update(len);
    hash.update(file.content);
  }
  return hash.digest("hex");
}

function writeCatalog() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "treq-skill-catalog-"));
  const skillMd = Buffer.from(
    "---\nname: demo\ndescription: demo skill\n---\n# Demo\n",
  );
  const skillPath = path.join(dir, "SKILL.md");
  fs.writeFileSync(skillPath, skillMd);
  const checksum = skillChecksum([{ path: "SKILL.md", content: skillMd }]);
  const catalogPath = path.join(dir, "catalog.json");
  fs.writeFileSync(
    catalogPath,
    JSON.stringify({
      generatedAt: new Date().toISOString(),
      sources: [{ id: "test", name: "Test" }],
      skills: [
        {
          id: "test/demo",
          name: "demo",
          description: "demo skill",
          source: "test",
          proprietary: false,
          checksum,
          files: [
            {
              path: "SKILL.md",
              size: skillMd.length,
              binary: false,
              rawUrl: `file://${skillPath}`,
            },
          ],
        },
      ],
    }),
  );
  process.env.TREQ_SKILLS_CATALOG_URL = catalogPath;
}

it("captures skill library browse and install", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  writeCatalog();
  await uninstallSkill("test/demo", repoPath).catch(() => undefined);

  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await user.click(await screen.findByRole("tab", { name: /skills/i }));
  expect(await screen.findByText("demo skill")).toBeTruthy();
  expect(await screen.findByText("0 installed")).toBeTruthy();

  await captureDocument(document, {
    name: "skill-library-01-browse",
    expectations: [
      "The Skills tab is selected and the demo skill shows a single Install… button.",
      "Search sits next to an Install level filter with All selected.",
    ],
  });

  await user.click(await screen.findByRole("button", { name: /^install/i }));
  expect(await screen.findByRole("dialog")).toBeTruthy();

  await captureDocument(document, {
    name: "skill-library-02-install-dialog",
    expectations: [
      "An Install demo dialog explains repository and application install levels.",
      "Install for repository is the filled primary button; Install for application is outline.",
    ],
  });

  await user.click(
    await screen.findByRole("button", { name: /install for repository/i }),
  );
  expect(await screen.findByLabelText(/install level for demo/i)).toHaveValue(
    "repository",
  );
  expect(await screen.findByText("1 installed")).toBeTruthy();

  await captureDocument(document, {
    name: "skill-library-03-installed",
    expectations: [
      "The demo card shows Install level set to Repository, aligned to the right.",
      "A trash icon uninstall control sits in the upper right of the card.",
    ],
  });

  await user.hover(await screen.findByRole("button", { name: /uninstall demo/i }));
  expect(await screen.findByRole("tooltip", { name: /uninstall/i })).toBeTruthy();
}, 60000);
