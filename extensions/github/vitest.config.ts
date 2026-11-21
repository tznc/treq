import { readFileSync } from "node:fs";
import { defineConfig } from "vitest/config";

export default defineConfig({
  // SVG icons are inlined as text, as `npm run build` does.
  plugins: [
    {
      name: "svg-text",
      enforce: "pre",
      load: (id) => (id.endsWith(".svg") ? `export default ${JSON.stringify(readFileSync(id, "utf8"))}` : null),
    },
  ],
  test: { environment: "happy-dom", include: ["src/**/*.test.ts"] },
});
