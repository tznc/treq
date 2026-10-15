import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures the auto-push toggle defaulting off and persisting once enabled", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	sendGpuiAction({ kind: "openSettings" });
	await screen.findByRole("tab", { name: /repository/i, selected: true });

	const autoPushSwitch = await screen.findByRole("switch", {
		name: /auto-push to remote/i,
	});
	expect(autoPushSwitch).toHaveAttribute("aria-checked", "false");

	await captureDocument(document, {
		name: "repository-settings-auto-push-01-default-off",
		expectations: [
			"The Repository tab shows an 'Auto-push to remote' row with a description below it.",
			"The Auto-push to remote switch is in the off (unchecked) position.",
		],
	});

	await user.click(autoPushSwitch);
	expect(autoPushSwitch).toHaveAttribute("aria-checked", "true");

	const headerSaveButton = screen.getByRole("button", { name: /save settings/i });
	await user.click(headerSaveButton);
	await screen.findByText("Settings saved");

	await captureDocument(document, {
		name: "repository-settings-auto-push-02-enabled-and-saved",
		expectations: [
			"The Auto-push to remote switch is in the on (checked) position.",
			"A 'Settings saved' toast is visible confirming the save.",
		],
	});

	await user.click(screen.getByRole("button", { name: /^close$/i }));
	sendGpuiAction({ kind: "openSettings" });
	await screen.findByRole("tab", { name: /repository/i, selected: true });

	const reloadedSwitch = await screen.findByRole("switch", {
		name: /auto-push to remote/i,
	});
	expect(reloadedSwitch).toHaveAttribute("aria-checked", "true");

	await captureDocument(document, {
		name: "repository-settings-auto-push-03-persisted-after-reopen",
		expectations: [
			"After closing and reopening Settings, the Auto-push to remote switch is still on.",
		],
	});
}, 60000);
