// A local skills catalog for the QA app (`TREQ_SKILLS_CATALOG_URL`), in
// place of treq.dev's: frontend-design, which 19-skill-link installs from
// a `treq://skill/install` link, and two more registry skills it leaves
// uninstalled. The app reads a catalog and its files from a plain path
// the same way it downloads them, so no network is needed.
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Skill = { id: string; name: string; description: string; files: Record<string, string> };

const skills: Skill[] = [
  {
    id: "anthropic/frontend-design",
    name: "frontend-design",
    description:
      "Create distinctive, production-grade frontend interfaces with high design quality.",
    files: {
      "SKILL.md":
        "---\nname: frontend-design\ndescription: Create distinctive, production-grade frontend interfaces with high design quality.\n---\n# Frontend design\n\nPick a clear aesthetic direction, then build the interface in real, working code.\n",
      "LICENSE.txt": "Apache License 2.0\n",
    },
  },
  {
    id: "anthropic/pdf",
    name: "pdf",
    description: "Read, fill and merge PDF files.",
    files: { "SKILL.md": "---\nname: pdf\ndescription: Read, fill and merge PDF files.\n---\n# PDF\n" },
  },
  {
    id: "anthropic/webapp-testing",
    name: "webapp-testing",
    description: "Test local web applications with Playwright.",
    files: {
      "SKILL.md":
        "---\nname: webapp-testing\ndescription: Test local web applications with Playwright.\n---\n# Web app testing\n",
    },
  },
];

/** `skill_checksum` in core/skills.rs: sorted paths, each NUL, length, bytes. */
function checksum(files: Record<string, string>): string {
  const hash = createHash("sha256");
  for (const path of Object.keys(files).sort()) {
    const bytes = Buffer.from(files[path]!, "utf8");
    const len = Buffer.alloc(8);
    len.writeBigUInt64BE(BigInt(bytes.length));
    hash.update(path).update(Buffer.from([0])).update(len).update(bytes);
  }
  return hash.digest("hex");
}

/** Writes the catalog and its files under `root`; returns the catalog's path. */
export function writeFakeSkills(root: string): string {
  const dir = join(root, "skills-catalog");
  const entries = skills.map((skill) => {
    const files = Object.entries(skill.files).map(([path, text]) => {
      const file = join(dir, skill.id, path);
      mkdirSync(join(file, ".."), { recursive: true });
      writeFileSync(file, text);
      return { path, size: Buffer.byteLength(text), binary: false, rawUrl: file };
    });
    return {
      id: skill.id,
      name: skill.name,
      description: skill.description,
      source: "anthropic",
      category: "Development",
      license: "Apache-2.0",
      url: `https://github.com/anthropics/skills/tree/main/skills/${skill.name}`,
      route: `/skills/${skill.id}`,
      checksum: checksum(skill.files),
      files,
    };
  });
  const catalog = join(dir, "catalog.json");
  writeFileSync(
    catalog,
    JSON.stringify({ generatedAt: "2026-08-01T00:00:00Z", sources: [], skills: entries }, null, 2),
  );
  return catalog;
}
