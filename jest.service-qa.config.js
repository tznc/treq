// Jest config for service-qa: specs under scripts/service-qa/specs run against
// the local Supabase CLI stack. Kept apart from `npm test` so the main suite
// does not need Docker. Babel here is Jest-only.
/** @type {import('jest').Config} */
export default {
  testEnvironment: "node",
  testMatch: ["<rootDir>/scripts/service-qa/specs/**/*.spec.ts"],
  setupFilesAfterEnv: ["<rootDir>/test/setup.service-qa.ts"],
  // One file at a time: specs share one local database.
  maxWorkers: 1,
  testTimeout: 60_000,
  transform: {
    "^.+\\.[jt]sx?$": [
      "babel-jest",
      {
        configFile: false,
        babelrc: false,
        presets: [
          ["@babel/preset-env", { targets: { node: "current" } }],
          "@babel/preset-typescript",
        ],
        plugins: ["babel-plugin-transform-import-meta"],
      },
    ],
  },
};
