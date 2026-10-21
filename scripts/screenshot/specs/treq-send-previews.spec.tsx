import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { TREQ_SEND_EVENT } from "../../../src/lib/treqSend";
import { captureDocument } from "../capture";
import fs from "node:fs";
import path from "node:path";
import { clickGpuiRow, openGpuiShell } from "../../../test/gpui-sidebar";

type SendPayload = {
  kind: string;
  request_id: string;
  repo: string;
  pty_session_id: string;
  media_type: string;
  path: string;
  title: string;
};

it("captures treq send attachment thumbs and lightbox carousel previews", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, "feat/treq-send");

  const notePath = path.join(repoPath, "preview-note.txt");
  fs.writeFileSync(
    notePath,
    "Selectable preview text from treq send.\nLine two.\n",
  );

  const imagePath = path.join(repoPath, "preview-shot.svg");
  const tallPath = path.join(repoPath, "preview-tall.svg");
  fs.writeFileSync(
    tallPath,
    `<svg xmlns="http://www.w3.org/2000/svg" width="80" height="640">
  <rect width="80" height="640" fill="#0f766e"/>
  <text x="40" y="40" text-anchor="middle" fill="#ecfdf5" font-size="18">TOP</text>
  <text x="40" y="620" text-anchor="middle" fill="#ecfdf5" font-size="18">BOT</text>
</svg>
`,
  );
  fs.writeFileSync(
    imagePath,
    `<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128">
  <rect width="128" height="128" fill="#2563eb"/>
  <circle cx="64" cy="64" r="36" fill="#f8fafc"/>
</svg>
`,
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow("feat/treq-send");
  await screen.findByTestId("workspace-terminal-pane");
  await openGpuiShell("feat/treq-send");

  await waitFor(() => {
    expect(
      document.querySelector('[data-terminal-id^="shell-"]'),
    ).not.toBeNull();
  });

  const terminalEl = document.querySelector(
    '[data-terminal-id^="shell-"]',
  ) as HTMLElement;
  const ptySessionId = terminalEl.getAttribute("data-terminal-id");
  expect(ptySessionId).toBeTruthy();

  await waitFor(() => {
    expect(
      vi.mocked(listen).mock.calls.some((args) => args[0] === TREQ_SEND_EVENT),
    ).toBe(true);
  });

  const sendCallback = vi
    .mocked(listen)
    .mock.calls.find((args) => args[0] === TREQ_SEND_EVENT)?.[1] as (event: {
    payload: SendPayload;
  }) => void;

  sendCallback({
    payload: {
      kind: "send",
      request_id: "qa-send-text",
      repo: repoPath,
      pty_session_id: ptySessionId!,
      media_type: "text",
      path: notePath,
      title: "preview-note.txt",
    },
  });
  sendCallback({
    payload: {
      kind: "send",
      request_id: "qa-send-image",
      repo: repoPath,
      pty_session_id: ptySessionId!,
      media_type: "image",
      path: imagePath,
      title: "preview-shot.svg",
    },
  });

  expect(
    await screen.findByTestId("terminal-send-preview-qa-send-text"),
  ).toBeTruthy();
  const imageThumb = await screen.findByTestId(
    "terminal-send-preview-qa-send-image",
  );
  expect(await screen.findByTestId("terminal-send-previews")).toBeTruthy();
  expect(
    screen
      .getByTestId("terminal-send-preview-qa-send-text")
      .getAttribute("draggable"),
  ).toBe("true");
  expect(
    imageThumb.closest('[data-slot="attachment"]')?.querySelector("img"),
  ).toBeTruthy();

  await captureDocument(document, {
    name: "treq-send-01-square-previews",
    expectations: [
      "Two dark landscape thumbnails (wider than tall, ~80x144) sit at the top-left of the terminal in a horizontal row.",
      "The image thumb shows a blue square with a white circle letterboxed (object-fit contain) inside the landscape card; the text thumb shows a file icon and title.",
      "Dismiss X controls are hidden until a thumbnail is hovered.",
    ],
  });

  await user.hover(
    screen
      .getByTestId("terminal-send-preview-qa-send-text")
      .closest('[data-slot="attachment"]') as HTMLElement,
  );
  expect(screen.getByTestId("terminal-send-dismiss-qa-send-text")).toBeTruthy();

  await captureDocument(document, {
    name: "treq-send-01a-hover-dismiss",
    expectations: [
      "Hovering the text attachment reveals its grey circular X dismiss control.",
      "The circular X overhangs the top-right corner of the thumbnail; its edges are not clipped flush with the card.",
      "The image attachment next to it still hides its X while not hovered.",
    ],
  });

  await user.click(screen.getByTestId("terminal-send-dismiss-qa-send-text"));
  await waitFor(() => {
    expect(
      screen.queryByTestId("terminal-send-preview-qa-send-text"),
    ).toBeNull();
  });

  await user.click(imageThumb);
  expect(await screen.findByTestId("treq-send-preview-lightbox")).toBeTruthy();
  expect(
    document.querySelector('[data-testid="treq-send-preview-lightbox"] img'),
  ).toBeTruthy();
  expect(screen.getByTestId("treq-send-zoom-out")).toBeTruthy();
  expect(screen.getByTestId("treq-send-zoom-in")).toBeTruthy();
  expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("100%");
  expect(screen.getByTestId("treq-send-copy")).toBeTruthy();
  expect(screen.getByTestId("treq-send-reveal")).toBeTruthy();
  expect(screen.getByTestId("treq-send-close")).toBeTruthy();
  expect(screen.queryByTestId("treq-send-preview-modal")).toBeNull();

  await captureDocument(document, {
    name: "treq-send-03-image-lightbox",
    expectations: [
      "At 100% zoom the blue SVG is large in the carousel (not a postage-stamp thumbnail).",
      "The carousel/content area spans at least three-quarters of the viewport; no small centered postage-stamp image.",
      "Centered above the image: filename preview-shot.svg, the zoom/copy/reveal/close toolbar, and a review comment input beside the toolbar.",
    ],
  });

  await captureDocument(document, {
    name: "treq-send-03a-carousel-nav",
    expectations: [
      "Circular previous/next carousel buttons are large (about 64px) with prominent arrows.",
      "The buttons sit near the left and right edges of the viewport, not in the gutter beside the image.",
    ],
  });

  await user.click(screen.getByTestId("treq-send-highlight-surface"));
  expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("200%");

  await captureDocument(document, {
    name: "treq-send-03b-image-zoomed",
    expectations: [
      "Toolbar zoom label reads 200% after clicking the image once to zoom.",
      "The blue SVG still fills the carousel frame (scrollable when larger than the shell).",
    ],
  });

  await user.click(screen.getByTestId("treq-send-close"));
  await waitFor(() => {
    expect(screen.queryByTestId("treq-send-preview-lightbox")).toBeNull();
  });

  await user.hover(
    screen
      .getByTestId("terminal-send-preview-qa-send-image")
      .closest('[data-slot="attachment"]') as HTMLElement,
  );
  await user.click(screen.getByTestId("terminal-send-dismiss-qa-send-image"));
  await waitFor(() => {
    expect(
      screen.queryByTestId("terminal-send-preview-qa-send-image"),
    ).toBeNull();
  });

  sendCallback({
    payload: {
      kind: "send",
      request_id: "qa-send-tall",
      repo: repoPath,
      pty_session_id: ptySessionId!,
      media_type: "image",
      path: tallPath,
      title: "preview-tall.svg",
    },
  });
  await user.click(
    await screen.findByTestId("terminal-send-preview-qa-send-tall"),
  );
  expect(await screen.findByTestId("treq-send-preview-lightbox")).toBeTruthy();
	await user.click(screen.getByTestId("treq-send-highlight-surface"));
  expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("200%");
  screen.getByTestId("treq-send-image-scroll").scrollTop = 0;

  await captureDocument(document, {
    name: "treq-send-03c-tall-zoomed",
    expectations: [
      "A tall teal photo is zoomed to 200% and the TOP label at the top of the image is visible (not cropped).",
      "The image is scrollable in the lightbox; preview-tall.svg is centered above the toolbar.",
    ],
  });

  await user.click(screen.getByTestId("treq-send-close"));
  await waitFor(() => {
    expect(screen.queryByTestId("treq-send-preview-lightbox")).toBeNull();
  });

  await user.hover(
    screen
      .getByTestId("terminal-send-preview-qa-send-tall")
      .closest('[data-slot="attachment"]') as HTMLElement,
  );
  await user.click(screen.getByTestId("terminal-send-dismiss-qa-send-tall"));
  await waitFor(() => {
    expect(
      screen.queryByTestId("terminal-send-preview-qa-send-image"),
    ).toBeNull();
  });

  sendCallback({
    payload: {
      kind: "send",
      request_id: "qa-send-text-2",
      repo: repoPath,
      pty_session_id: ptySessionId!,
      media_type: "text",
      path: notePath,
      title: "preview-note.txt",
    },
  });
  await user.click(
    await screen.findByTestId("terminal-send-preview-qa-send-text-2"),
  );
  expect(await screen.findByTestId("treq-send-preview-lightbox")).toBeTruthy();
  await waitFor(() => {
    expect(
      screen.getByText(/Selectable preview text from treq send/),
    ).toBeTruthy();
  });
  expect(
    screen.getByTestId("treq-send-preview-lightbox").textContent,
  ).toContain("preview-note.txt");
  expect(screen.queryByTestId("treq-send-zoom-in")).toBeNull();

  await captureDocument(document, {
    name: "treq-send-02-text-lightbox",
    expectations: [
      "A blurred backdrop shows the text asset alone without a modal frame.",
      "The selectable text includes 'Selectable preview text from treq send'.",
      "Copy, reveal, and close sit in a centered toolbar under the filename — no zoom controls for text.",
    ],
  });
}, 60000);
