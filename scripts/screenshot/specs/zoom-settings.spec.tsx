import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { useZoomSettingsStore } from "../../../src/stores/zoomSettingsStore";
import { captureDocument } from "../capture";
import { gpuiSnapshot, sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures UI zoom settings slider and keyboard zoom", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);
	document.documentElement.style.zoom = "100%";

	const user = userEvent.setup();
	render(<Dashboard />);

	await waitFor(() => expect(gpuiSnapshot()).toBeTruthy());
	await captureDocument(document, {
		name: "zoom-settings-01-default",
		expectations: [
			"The dashboard renders at normal scale before any zoom change.",
		],
	});

	sendGpuiAction({ kind: "openSettings" });
	await user.click(await screen.findByRole("tab", { name: /application/i }));
	const slider = await screen.findByRole("slider");
	expect(slider).toHaveAttribute("aria-valuenow", "100");

	await captureDocument(document, {
		name: "zoom-settings-02-slider",
		expectations: [
			"The Application settings tab is open with a UI Zoom slider.",
			"The zoom value label reads 100%.",
		],
	});

	slider.focus();
	await user.keyboard("{ArrowRight}{ArrowRight}");
	expect(slider).toHaveAttribute("aria-valuenow", "110");

	await user.click(screen.getByRole("button", { name: /save settings/i }));
	await screen.findByText("Settings Saved");
	// Zoom is applied through Tauri's webview.setZoom, never through a CSS zoom
	// on documentElement -- see test/integration/zoom-settings.test.tsx, which
	// asserts documentElement stays untouched. Check the store instead.
	expect(useZoomSettingsStore.getState().zoom).toBe(110);
	expect(document.documentElement.style.zoom).toBe("100%");

	await captureDocument(document, {
		name: "zoom-settings-03-after-save",
		expectations: [
			"The settings page still shows the Application tab after saving.",
			"The UI Zoom slider reflects 110%.",
		],
	});
	slider.focus();
	await user.keyboard("{Home}");
	expect(slider).toHaveAttribute("aria-valuenow", "50");
	await captureDocument(document, {
		name: "zoom-settings-04-min",
		expectations: [
			"The UI Zoom value label reads 50%.",
			"The blue filled portion of the slider track is empty at the far left.",
			"The circular thumb sits at the left end of the track, on the fill edge.",
		],
	});

	await user.keyboard("{End}");
	expect(slider).toHaveAttribute("aria-valuenow", "200");
	await captureDocument(document, {
		name: "zoom-settings-05-max",
		expectations: [
			"The UI Zoom value label reads 200%.",
			"The blue filled portion spans the full slider track.",
			"The circular thumb sits at the right end of the track, on the fill edge.",
		],
	});
}, 60000);
