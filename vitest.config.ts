import { defineConfig } from "vitest/config";
import { unitProjects } from "./vitest.unit.projects";

/**
 * Default vitest entry: runs unit + integration as separate projects.
 * Prefer `npm run test:unit` / `npm run test:integration` in CI so setup
 * cost (NAPI build, jj) is only paid where needed.
 *
 * Lists the unit and integration projects directly rather than via
 * vitest.unit.config.ts / vitest.integration.config.ts -- Vitest doesn't
 * support a project entry that itself declares `projects`.
 */
export default defineConfig({
  test: {
    projects: [
      ...unitProjects,
      "./vitest.integration-serial.config.ts",
      "./vitest.integration-parallel.config.ts",
    ],
  },
});
