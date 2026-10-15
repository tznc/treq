import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo, writeRepoFile } from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures the repository tab with no config, synced with fields disabled, and after a reload", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	sendGpuiAction({ kind: "openSettings" });
	await screen.findByRole("tab", { name: /repository/i, selected: true });

	const branchPatternInput = await screen.findByLabelText(/branch name pattern/i);
	expect(branchPatternInput).not.toBeDisabled();
	expect(
		screen.queryByTestId("repo-yaml-config-card"),
	).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "repo-yaml-config-sync-01-no-config",
		expectations: [
			"No sync card is shown at the top of the Repository tab.",
			"Branch Name Pattern, Included Files/Directories, Claude Code Model, and Default Agent all look like normal, enabled (not greyed out) fields.",
		],
	});

	await user.click(screen.getByRole("button", { name: /^close$/i }));

	await writeRepoFile(
		repoPath,
		".treq/config.yaml",
		["default_model: opus", "default_agent: claude"].join("\n"),
	);

	sendGpuiAction({ kind: "openSettings" });
	await screen.findByRole("tab", { name: /repository/i, selected: true });

	await screen.findByText(/synced from \.treq\/config\.yaml/i);
	const modelSelect = await screen.findByLabelText(/claude code model/i);
	expect(modelSelect).toBeDisabled();

	await captureDocument(document, {
		name: "repo-yaml-config-sync-02-synced-fields-disabled",
		expectations: [
			"A card at the top of the Repository tab reads 'Synced from .treq/config.yaml' with an explanation that disabled settings below are managed by that file, plus a Reload button.",
			"Claude Code Model and Default Agent are visibly disabled (greyed out), while Branch Name Pattern above them is not.",
		],
		scrollIntoView: '[data-testid="repo-yaml-config-card"]',
	});

	await writeRepoFile(
		repoPath,
		".treq/config.yaml",
		'branch_name_pattern: "release/{name}"\n',
	);

	await user.click(await screen.findByRole("button", { name: /reload/i }));
	await waitFor(() => {
		const branchPatternAfterReload = screen.getByLabelText(
			/branch name pattern/i,
		);
		expect(branchPatternAfterReload).toBeDisabled();
		expect(branchPatternAfterReload).toHaveValue("release/{name}");
		expect(screen.getByLabelText(/claude code model/i)).not.toBeDisabled();
		expect(screen.getByLabelText(/default agent/i)).not.toBeDisabled();
	});

	await captureDocument(document, {
		name: "repo-yaml-config-sync-03-after-reload",
		expectations: [
			"Branch Name Pattern now shows the value release/{name} and is greyed out/disabled, since the reloaded file added branch_name_pattern.",
			"Claude Code Model and Default Agent are no longer disabled and show their normal editable appearance, since the reloaded file dropped default_model and default_agent.",
		],
		scrollIntoView: '[data-testid="repo-yaml-config-card"]',
	});
}, 60000);
