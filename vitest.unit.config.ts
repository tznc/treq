import { defineConfig } from "vitest/config";
import { unitProjects } from "./vitest.unit.projects";

export default defineConfig({
  test: { projects: unitProjects },
});
