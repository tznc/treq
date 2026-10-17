import { test, expect } from "@playwright/test";

// ── Skills Library (/skills) ────────────────────────────────────────────────

test.describe("Skills Library (/skills)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page
      .getByRole("navigation")
      .getByRole("link", { name: "Skills" })
      .click();
  });

  test("renders page heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Skills Library", level: 1 }),
    ).toBeVisible();
  });

  test("lists skill cards from curated sources", async ({ page }) => {
    await expect(page.getByRole("link", { name: /pdf/ }).first()).toBeVisible();
  });

  test("searching narrows the results", async ({ page }) => {
    const search = page.getByRole("searchbox", { name: "Search skills" });
    await search.fill("tdd");
    await expect(page.getByText(/\d+ of \d+ skills/)).toBeVisible();
    await expect(page.getByRole("link", { name: /^tdd/ })).toBeVisible();
  });

  test("an unmatched search shows the empty state", async ({ page }) => {
    await page
      .getByRole("searchbox", { name: "Search skills" })
      .fill("zzzz-not-a-real-skill");
    await expect(page.getByText("No skills match your search.")).toBeVisible();
  });

  test("the provider dropdown multi-selects and filters", async ({ page }) => {
    await page.getByRole("button", { name: /All providers/ }).click();
    await page.getByRole("checkbox", { name: /Matt Pocock/ }).check();
    // Close the dropdown so it doesn't overlay the results.
    await page
      .getByRole("heading", { name: "Skills Library", level: 1 })
      .click();
    await expect(
      page.getByRole("button", { name: "1 provider" }),
    ).toBeVisible();
    await expect(page.getByRole("link", { name: /^tdd/ })).toBeVisible();
    await expect(page.getByRole("link", { name: /^pdf/ })).toHaveCount(0);
  });

  test("skill cards navigate to the skill detail page", async ({ page }) => {
    const card = page.getByRole("link", { name: /^pdf/ }).first();
    await expect(card).toHaveAttribute("href", "/skills/anthropic/pdf");
    await expect(card).not.toHaveAttribute("target", "_blank");
  });

  test("proprietary skills show a notice on their card instead of a description", async ({
    page,
  }) => {
    const card = page.getByRole("link", { name: /^pdf/ }).first();
    await expect(card).toContainText("Proprietary license");
    await expect(card).toContainText("Proprietary");
  });
});

// ── Ranked source-repos aside ───────────────────────────────────────────────

test.describe("Source repos aside", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page
      .getByRole("navigation")
      .getByRole("link", { name: "Skills" })
      .click();
  });

  test("lists source repos ranked by stars", async ({ page }) => {
    const aside = page.getByRole("complementary", {
      name: /Source repositories by stars/,
    });
    await expect(aside.getByText("Source repos")).toBeVisible();
    const topRepo = aside.getByRole("listitem").first();
    await expect(
      topRepo.getByRole("button", { name: /Superpowers/ }),
    ).toBeVisible();
    await expect(topRepo).toContainText(/★ \d/);
  });

  test("clicking a repo in the aside filters the catalog by that provider", async ({
    page,
  }) => {
    const aside = page.getByRole("complementary", {
      name: /Source repositories by stars/,
    });
    await aside.getByRole("button", { name: /Microsoft/ }).click();
    await expect(
      page.getByRole("button", { name: "1 provider" }),
    ).toBeVisible();
    await expect(
      page.getByRole("link", { name: /^azure-ai Microsoft/ }),
    ).toBeVisible();
    await expect(page.getByRole("link", { name: /^pdf/ })).toHaveCount(0);
  });
});

// ── Skill detail page (file browser) ────────────────────────────────────────

test.describe("Skill detail page", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page
      .getByRole("navigation")
      .getByRole("link", { name: "Skills" })
      .click();
    await page
      .getByRole("link", { name: /^mcp-builder/ })
      .first()
      .click();
  });

  test("renders skill heading and repo GitHub link", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "mcp-builder", level: 1 }),
    ).toBeVisible();
    await expect(page.getByTestId("repo-github-link")).toHaveAttribute(
      "href",
      "https://github.com/anthropics/skills/tree/main/skills/mcp-builder",
    );
  });

  test("offers an Open in Treq deep link for the skill", async ({ page }) => {
    await expect(
      page.getByRole("link", { name: "Open in Treq" }),
    ).toHaveAttribute("href", "treq://skill/install?id=anthropic%2Fmcp-builder");
  });

  test("file tree lists the skill files", async ({ page }) => {
    await expect(page.getByRole("button", { name: "SKILL.md" })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "mcp_best_practices.md" }),
    ).toBeVisible();
  });

  test("SKILL.md is selected by default and its content loads", async ({
    page,
  }) => {
    // Content is fetched live from GitHub's raw CDN, so give the network
    // request more headroom than the default UI-interaction assertions.
    test.setTimeout(30_000);
    await expect(page.getByTestId("viewer-path")).toHaveText("SKILL.md");
    await expect(page.getByTestId("viewer-body")).toContainText("MCP", {
      timeout: 20_000,
    });
  });

  test("clicking another file swaps the preview and its GitHub link", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "mcp_best_practices.md" }).click();
    await expect(page.getByTestId("viewer-path")).toHaveText(
      "reference/mcp_best_practices.md",
    );
    await expect(page.getByTestId("viewer-github-link")).toHaveAttribute(
      "href",
      "https://github.com/anthropics/skills/blob/main/skills/mcp-builder/reference/mcp_best_practices.md",
    );
  });

  test("breadcrumb link returns to the Skills Library", async ({ page }) => {
    await page.getByRole("link", { name: "Skills Library" }).click();
    await expect(
      page.getByRole("heading", { name: "Skills Library", level: 1 }),
    ).toBeVisible();
  });

  test("shows a detected license label", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "mcp-builder", level: 1 }),
    ).toBeVisible();
    await expect(page.getByTestId("license-badge")).toHaveText("Apache-2.0");
  });

  test("markdown files default to a Preview tab, with Code as a second tab", async ({
    page,
  }) => {
    test.setTimeout(30_000);
    const tabs = page.getByRole("tablist");
    await expect(tabs.getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(tabs.getByRole("tab", { name: "Code" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    const viewerBody = page.getByTestId("viewer-body");
    // The rendered SKILL.md has several h1s; any one being visible proves the
    // markdown was rendered rather than shown as raw source.
    await expect(
      viewerBody.getByRole("heading", { level: 1 }).first(),
    ).toBeVisible({ timeout: 20_000 });
  });

  test("switching to the Code tab shows the raw markdown source", async ({
    page,
  }) => {
    test.setTimeout(30_000);
    const viewerBody = page.getByTestId("viewer-body");
    await expect(
      viewerBody.getByRole("heading", { level: 1 }).first(),
    ).toBeVisible({ timeout: 20_000 });
    await page.getByRole("tab", { name: "Code" }).click();
    await expect(page.getByRole("tab", { name: "Code" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(viewerBody.getByRole("heading", { level: 1 })).toHaveCount(0);
    await expect(viewerBody).toContainText("#");
  });
});

// ── Proprietary skill detail page ───────────────────────────────────────────

test.describe("Proprietary skill detail page", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page
      .getByRole("navigation")
      .getByRole("link", { name: "Skills" })
      .click();
    await page.getByRole("link", { name: /^pdf/ }).first().click();
  });

  test("shows a proprietary callout, no file browser, but keeps the GitHub link", async ({
    page,
  }) => {
    await expect(
      page.getByRole("heading", { name: "pdf", level: 1 }),
    ).toBeVisible();
    await expect(page.getByTestId("proprietary-callout")).toBeVisible();
    await expect(page.getByTestId("skill-file-browser")).toHaveCount(0);
    await expect(page.getByRole("link", { name: "Open in Treq" })).toHaveCount(
      0,
    );
    await expect(page.getByTestId("repo-github-link")).toHaveAttribute(
      "href",
      "https://github.com/anthropics/skills/tree/main/skills/pdf",
    );
  });
});

// ── Skill detail page — mobile ──────────────────────────────────────────────

test.describe("Skill detail page on mobile", () => {
  test.use({ viewport: { width: 375, height: 667 } });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // The navbar collapses to a hamburger at this width.
    await page.getByLabel("Toggle navigation bar").click();
    await page
      .getByRole("navigation", { name: "Main" })
      .getByRole("link", { name: "Skills", exact: true })
      .filter({ visible: true })
      .click();
    await page
      .getByRole("link", { name: /^mcp-builder/ })
      .first()
      .click();
  });

  test("shows a file dropdown instead of the folder tree", async ({ page }) => {
    await expect(page.getByLabel("Choose a file")).toBeVisible();
    await expect(
      page.getByRole("button", { name: "SKILL.md" }),
    ).not.toBeVisible();
  });

  test("picking a file from the dropdown updates the preview", async ({
    page,
  }) => {
    await expect(page.getByTestId("viewer-path")).toHaveText("SKILL.md");
    await page
      .getByLabel("Choose a file")
      .selectOption("reference/mcp_best_practices.md");
    await expect(page.getByTestId("viewer-path")).toHaveText(
      "reference/mcp_best_practices.md",
    );
  });
});
