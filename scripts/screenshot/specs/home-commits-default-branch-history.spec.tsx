import * as React from "react";
import { execSync } from "node:child_process";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	gitCommitRepoFile,
	openRepo,
} from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

// Scenario: the home repo is checked out on a feature branch cut from the
// default branch tip. The Commits tab lists the branch's own commits, then a
// "Recent on <default>" divider with the default branch's history below it.
it("captures default branch history in the home repo Commits tab", async () => {
	const { repoPath, defaultBranch } = createTestRepo(false);
	openRepo(repoPath);

	for (let i = 1; i <= 3; i++) {
		await gitCommitRepoFile(
			repoPath,
			`main-file-${i}.txt`,
			`main content ${i}`,
			`Main commit ${i}`,
		);
	}
	execSync("git checkout -b chore/update-deps-audit", { cwd: repoPath });
	await gitCommitRepoFile(
		repoPath,
		"deps-1.txt",
		"deps 1",
		"chore: update deps for vulnerabilities",
	);
	await gitCommitRepoFile(
		repoPath,
		"deps-2.txt",
		"deps 2",
		"chore: update deps to fix tailwind",
	);

	const user = userEvent.setup();
	render(<Dashboard />);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Commits/ }));

	await screen.findByText("chore: update deps to fix tailwind");
	await screen.findByText(`Recent on ${defaultBranch}`);
	await screen.findByText("Main commit 3");

	await captureDocument(document, {
		name: "home-commits-default-branch-history-01-commits-tab",
		expectations: [
			"The two chore: branch commits appear at the top of the Commits tab.",
			`A divider labelled "Recent on ${defaultBranch}" sits below them, followed by Main commit 3, 2 and 1 in faded style.`,
			"Day headers such as Today are rendered in normal weight, not bold.",
		],
	});

	await user.click(await screen.findByRole("tab", { name: /^Code/ }));
	await screen.findByText(/^Diverged from/);
	await captureDocument(document, {
		name: "home-commits-default-branch-history-02-code-sidebar",
		expectations: [
			"The Code tab's right sidebar lists the branch commits, then a Diverged from divider.",
			"Default branch commits (Main commit 3, 2, 1) appear faded below the divider.",
		],
	});
}, 120000);
