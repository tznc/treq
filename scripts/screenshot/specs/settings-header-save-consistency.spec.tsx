import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures repository tab saving via the shared header button", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	sendGpuiAction({ kind: "openSettings" });
	await screen.findByRole("tab", { name: /repository/i, selected: true });

	const branchPatternInput = await screen.findByLabelText(/branch name pattern/i);
	expect(
		screen.queryByRole("button", { name: /^cancel$/i }),
	).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "settings-header-save-01-repository-tab",
		expectations: [
			"The Settings page is open on the Repository tab.",
			"The header (top right, next to Close) shows a single Save Settings button.",
			"There is no second Save Settings or Cancel button at the bottom of the Repository tab content.",
		],
	});

	await user.clear(branchPatternInput);
	await user.type(branchPatternInput, "custom/{{name}}");

	const headerSaveButton = screen.getByRole("button", { name: /save settings/i });
	await user.click(headerSaveButton);
	await screen.findByText("Settings saved");

	await captureDocument(document, {
		name: "settings-header-save-02-repository-saved",
		expectations: [
			"A 'Settings saved' toast confirms the repository settings were saved via the header button.",
			"The Branch Name Pattern field shows the updated value custom/{name} (literal curly braces).",
		],
	});

	await user.click(await screen.findByRole("tab", { name: /application/i }));
	await screen.findByRole("tab", { name: /application/i, selected: true });
	const appHeaderSaveButton = await screen.findByRole("button", {
		name: /save settings/i,
	});
	await user.click(appHeaderSaveButton);
	await screen.findByText("Settings Saved");

	await captureDocument(document, {
		name: "settings-header-save-03-application-saved",
		expectations: [
			"The Application tab is active and shows a 'Settings Saved' toast after clicking the header Save Settings button.",
			"The header still shows Close and Save Settings buttons, matching the Repository tab's header layout.",
		],
	});
}, 60000);
