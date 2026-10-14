import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

it("captures the app-wide type scale on commits and settings", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
	await screen.findByText("Today");
	await captureDocument(document, {
		name: "typography-scale-01-commits",
		expectations: [
			"Commit titles are the same size as sidebar labels (13px body), not larger.",
			"Tab labels, badges and timestamps read as one consistent dense scale.",
		],
	});

	await user.click(await screen.findByLabelText("Settings"));
	await user.click(await screen.findByRole("tab", { name: /application/i }));
	await screen.findByLabelText("Font Size");
	await captureDocument(document, {
		name: "typography-scale-02-settings",
		expectations: [
			"Select values, labels and nav items share the 13px body size.",
			"Help text under each field is smaller than the field labels.",
		],
	});
}, 60000);
