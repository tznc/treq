import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

it("captures the UI zoom slider and font size presets", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	await user.click(await screen.findByLabelText("Settings"));
	await user.click(await screen.findByRole("tab", { name: /application/i }));
	const thumb = await screen.findByRole("slider");
	await captureDocument(document, {
		name: "settings-zoom-font-presets-01-default",
		expectations: [
			"The UI Zoom slider shows a full round thumb (not a clipped sliver) at 100%.",
			"Font Size is a dropdown reading 'Medium (default)', not a number input.",
		],
	});

	thumb.focus();
	await user.keyboard("{ArrowRight}{ArrowRight}");
	await screen.findByText("110%");
	await user.selectOptions(screen.getByLabelText("Font Size"), "Large");
	await captureDocument(document, {
		name: "settings-zoom-font-presets-02-changed",
		expectations: [
			"UI Zoom reads 110% and the thumb has moved right of its previous spot.",
			"Font Size dropdown reads 'Large'.",
		],
	});
}, 60000);
