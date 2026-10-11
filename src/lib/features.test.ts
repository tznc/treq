import { describe, expect, it } from "vitest";
import {
  FEATURES,
  PREVIEW_FEATURES,
  parsePreviewSetting,
  previewFeatureDefault,
  previewSettingKey,
} from "./features";

describe("preview features", () => {
  it("lists all preview features with docs paths", () => {
    expect(PREVIEW_FEATURES.map((feature) => feature.id)).toEqual([
      "skillsInstallation",
      "workspaceScheduling",
      "remoteSsh",
      "linearIntegration",
      "trelloIntegration",
      "jiraIntegration",
      "logs",
      "browser",
      "agentMessageQueue",
    ]);
    for (const feature of PREVIEW_FEATURES) {
      expect(feature.docsPath.startsWith("/docs/")).toBe(true);
      expect(feature.title.length).toBeGreaterThan(0);
    }
  });

  it("uses package.json as the production default", () => {
    expect(FEATURES.skillsInstallation).toBe(false);
    expect(FEATURES.workspaceScheduling).toBe(false);
    expect(FEATURES.remoteSsh).toBe(false);
    expect(FEATURES.linearIntegration).toBe(false);
    expect(FEATURES.trelloIntegration).toBe(false);
    expect(FEATURES.jiraIntegration).toBe(false);
    expect(FEATURES.logs).toBe(false);
    expect(FEATURES.browser).toBe(false);
    expect(FEATURES.agentMessageQueue).toBe(false);
  });

  it("defaults every preview flag on in test and dev", () => {
    expect(previewFeatureDefault("skillsInstallation")).toBe(true);
    expect(previewFeatureDefault("workspaceScheduling")).toBe(true);
    expect(previewFeatureDefault("remoteSsh")).toBe(true);
    expect(previewFeatureDefault("linearIntegration")).toBe(true);
    expect(previewFeatureDefault("trelloIntegration")).toBe(true);
    expect(previewFeatureDefault("jiraIntegration")).toBe(true);
    expect(previewFeatureDefault("logs")).toBe(true);
    expect(previewFeatureDefault("browser")).toBe(true);
    expect(previewFeatureDefault("agentMessageQueue")).toBe(true);
  });

  it("honors stored true/false over the startup default", () => {
    expect(parsePreviewSetting("false", "remoteSsh")).toBe(false);
    expect(parsePreviewSetting("true", "remoteSsh")).toBe(true);
    expect(parsePreviewSetting(null, "remoteSsh")).toBe(
      previewFeatureDefault("remoteSsh"),
    );
  });

  it("stores overrides under feature_preview.<id>", () => {
    expect(previewSettingKey("skillsInstallation")).toBe(
      "feature_preview.skillsInstallation",
    );
  });
});
