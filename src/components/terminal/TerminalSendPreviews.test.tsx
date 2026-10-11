import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TerminalSendPreviews } from "./TerminalSendPreviews";
import { ToastProvider } from "../ui/toast";
import { AppStoreEffects } from "../../stores/AppStoreEffects";
import { useTreqSendStore } from "../../stores/treqSendStore";
import { TREQ_SEND_EVENT } from "../../lib/treqSend";
import * as api from "../../lib/api";
import * as treqSend from "../../lib/treqSend";
import * as utils from "../../lib/utils";
import * as sendAssetDrag from "../../lib/send-asset-drag";
import { SEND_ASSET_MIME } from "../../lib/send-asset-drag";
import * as sendAssetReview from "../../lib/sendAssetReview";

vi.spyOn(api, "readFile").mockResolvedValue("hello from send");
vi.spyOn(treqSend, "treqSendFileSrc").mockImplementation(
  (path: string) => `asset://localhost${path}`,
);
vi.spyOn(utils, "copyTextToClipboard").mockResolvedValue(undefined);
vi.spyOn(sendAssetDrag, "copySendAssetToClipboard").mockResolvedValue(
  undefined,
);

function SendHarness({
  ptySessionId,
  isActive = true,
  onSendReview,
  onInsertIntoTerminal,
}: {
  ptySessionId: string;
  isActive?: boolean;
  onSendReview?: (prompt: string) => void;
  onInsertIntoTerminal?: (text: string) => void;
}) {
  const ingestPayload = useTreqSendStore((s) => s.ingestPayload);
  return (
    <div>
      <button
        type="button"
        onClick={() =>
          ingestPayload({
            kind: "send",
            request_id: "send-1",
            repo: "/tmp/repo",
            pty_session_id: ptySessionId,
            media_type: "text",
            path: "/tmp/repo/.treq/send/note.txt",
            title: "note.txt",
          })
        }
      >
        Inject text
      </button>
      <button
        type="button"
        onClick={() =>
          ingestPayload({
            kind: "send",
            request_id: "send-2",
            repo: "/tmp/repo",
            pty_session_id: ptySessionId,
            media_type: "image",
            path: "/tmp/repo/shot.png",
            title: "shot.png",
          })
        }
      >
        Inject image
      </button>
      <TerminalSendPreviews
        ptySessionId={ptySessionId}
        isActive={isActive}
        onSendReview={onSendReview}
        onInsertIntoTerminal={onInsertIntoTerminal}
      />
    </div>
  );
}

function renderSend(ui: ReactElement) {
  return render(
    <ToastProvider>
      <AppStoreEffects />
      {ui}
    </ToastProvider>,
  );
}

describe("TerminalSendPreviews", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.readFile).mockResolvedValue("hello from send");
    vi.mocked(treqSend.treqSendFileSrc).mockImplementation(
      (path: string) => `asset://localhost${path}`,
    );
    vi.mocked(utils.copyTextToClipboard).mockResolvedValue(undefined);
    vi.spyOn(sendAssetDrag, "copySendAssetToClipboard").mockResolvedValue(
      undefined,
    );
  });

  it("shows attachment thumbnails and opens a lightbox text preview on click", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);

    await user.click(screen.getByRole("button", { name: "Inject text" }));
    expect(await screen.findByTestId("terminal-send-previews")).toBeTruthy();
    const thumb = await screen.findByTestId("terminal-send-preview-send-1");
    await user.click(thumb);

    expect(
      await screen.findByTestId("treq-send-preview-lightbox"),
    ).toBeTruthy();
    await waitFor(() => {
      expect(
        screen.getByTestId("treq-send-text-preview").textContent,
      ).toContain("hello from send");
    });
    expect(screen.getByTestId("treq-send-text-preview").className).toContain(
      "bg-zinc-900",
    );
    expect(screen.getByTestId("treq-send-text-preview").className).toContain(
      "opacity-100",
    );
    expect(screen.getByTestId("treq-send-text-preview").className).not.toMatch(
      /bg-zinc-900\/\d+/,
    );
    expect(screen.getByTestId("treq-send-preview-lightbox").className).toMatch(
      /fixed inset-0/,
    );
    expect(
      screen.getByRole("button", { name: "Close preview backdrop" }).className,
    ).toMatch(/bg-black\/\d+/);
    expect(screen.queryByTestId("treq-send-preview-modal")).toBeNull();
    expect(api.readFile).toHaveBeenCalledWith("/tmp/repo/.treq/send/note.txt");
  });

  it("renders image thumbnails with asset URLs", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    const thumb = await screen.findByTestId("terminal-send-preview-send-2");
    const attachment = thumb.closest('[data-slot="attachment"]');
    expect(attachment?.className).toMatch(/h-20/);
    expect(attachment?.className).toMatch(/w-36/);
    const img = attachment?.querySelector("img");
    expect(img?.className).toMatch(/object-contain/);
    expect(img?.getAttribute("src")).toBe(
      "asset://localhost/tmp/repo/shot.png",
    );
  });

  it("lets the dismiss button overhang the thumbnail corner", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    const thumb = await screen.findByTestId("terminal-send-preview-send-1");
    const attachment = thumb.closest('[data-slot="attachment"]') as HTMLElement;
    const group = thumb.closest(
      '[data-slot="attachment-group"]',
    ) as HTMLElement;
    const actions = attachment.querySelector(
      '[data-slot="attachment-actions"]',
    ) as HTMLElement;

    expect(attachment.className).toMatch(/overflow-visible/);
    expect(actions.className).toMatch(/-top-2/);
    expect(actions.className).toMatch(/-right-2/);
    expect(group.className).toMatch(/pt-2/);
    expect(group.className).toMatch(/pr-2/);
  });

  it("dismisses a thumbnail when its x button is clicked", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    const thumb = await screen.findByTestId("terminal-send-preview-send-1");
    const attachment = thumb.closest('[data-slot="attachment"]') as HTMLElement;
    await user.hover(attachment);
    await user.click(screen.getByTestId("terminal-send-dismiss-send-1"));
    await waitFor(() => {
      expect(screen.queryByTestId("terminal-send-preview-send-1")).toBeNull();
      expect(screen.queryByTestId("terminal-send-previews")).toBeNull();
    });
  });

  it("copies text assets and reveals them in the file manager", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-1"));
    await screen.findByTestId("treq-send-preview-lightbox");

    await user.click(screen.getByTestId("treq-send-copy"));
    await waitFor(() => {
      expect(sendAssetDrag.copySendAssetToClipboard).toHaveBeenCalledWith({
        path: "/tmp/repo/.treq/send/note.txt",
        title: "note.txt",
      });
    });

    await user.click(screen.getByTestId("treq-send-reveal"));
    expect(revealItemInDir).toHaveBeenCalledWith(
      "/tmp/repo/.treq/send/note.txt",
    );

    await user.click(screen.getByTestId("treq-send-close"));
    await waitFor(() => {
      expect(screen.queryByTestId("treq-send-preview-lightbox")).toBeNull();
    });
  });

  it("opens image lightbox with viewport-scaled carousel and base width", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-2"));
    await screen.findByTestId("treq-send-preview-lightbox");

    const carouselShell = screen.getByTestId(
      "treq-send-preview-carousel-shell",
    );
    expect(carouselShell.className).toMatch(/w-\[90vw\]/);
    expect(carouselShell.className).toMatch(/min-w-\[75vw\]/);

    const image = screen
      .getByTestId("treq-send-preview-lightbox")
      .querySelector('img[alt="shot.png"]') as HTMLImageElement;
    expect(image).toBeTruthy();
    expect(image.style.height).toBe("80vh");
    expect(image.style.width).toBe("auto");
  });

  it("zooms image assets in and out from the lightbox toolbar", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-2"));
    await screen.findByTestId("treq-send-preview-lightbox");

    const zoomIn = screen.getByTestId("treq-send-zoom-in");
    const zoomOut = screen.getByTestId("treq-send-zoom-out");
    expect(zoomIn).toBeTruthy();
    expect(zoomOut).toBeTruthy();

    const image = screen
      .getByTestId("treq-send-preview-lightbox")
      .querySelector('img[alt="shot.png"]') as HTMLImageElement;
    expect(image).toBeTruthy();
    expect(image.style.height).toBe("80vh");

    await user.click(zoomIn);
    expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("200%");
    expect(image.style.height).toBe("160vh");

    await user.click(zoomOut);
    expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("100%");
    expect(image.style.height).toBe("80vh");
  });

  it("toggles one zoom level when the lightbox image is clicked", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-2"));
    await screen.findByTestId("treq-send-preview-lightbox");

    const image = screen
      .getByTestId("treq-send-preview-lightbox")
      .querySelector('img[alt="shot.png"]') as HTMLImageElement;
    expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("100%");

    await user.click(screen.getByTestId("treq-send-highlight-surface"));
    expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("200%");
    expect(image.style.height).toBe("160vh");
    expect(screen.getByTestId("treq-send-preview-lightbox")).toBeTruthy();

    await user.click(screen.getByTestId("treq-send-highlight-surface"));
    expect(screen.getByTestId("treq-send-zoom-level").textContent).toBe("100%");
    expect(image.style.height).toBe("80vh");
  });

  it("centers the file name above the lightbox toolbar", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-2"));
    const header = await screen.findByTestId("treq-send-preview-header");
    expect(header.className).toMatch(/flex-col/);
    expect(header.className).toMatch(/items-center/);
    const title = header.querySelector("p");
    expect(title?.textContent).toBe("shot.png");
    expect(header.firstElementChild).toBe(title);
    expect(
      header.querySelector('[data-testid="treq-send-zoom-in"]'),
    ).toBeTruthy();
    expect(
      header.querySelector('[data-testid="treq-send-close"]'),
    ).toBeTruthy();
    expect(
      screen
        .getByTestId("treq-send-preview-lightbox")
        .querySelector(":scope > .absolute.right-4.top-4"),
    ).toBeNull();
  });

  it("uses a scrollable image frame so tall zoomed photos are not cropped at the top", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-2"));
    await screen.findByTestId("treq-send-preview-lightbox");

    const frame = screen.getByTestId("treq-send-image-scroll");
    expect(frame.className).toMatch(/overflow-auto/);
    expect(frame.className).not.toMatch(/items-center/);
    expect(frame.querySelector(".min-h-full")).toBeTruthy();

    await user.click(screen.getByTestId("treq-send-zoom-in"));
    expect(frame.className).toMatch(/h-\[80vh\]/);
    expect(frame.querySelector(".items-start")).toBeTruthy();
  });

  it("hides zoom controls for text assets", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-1"));
    await screen.findByTestId("treq-send-preview-lightbox");

    expect(screen.queryByTestId("treq-send-zoom-in")).toBeNull();
    expect(screen.queryByTestId("treq-send-zoom-out")).toBeNull();
  });

  it("listens for treq-send-received events", async () => {
    renderSend(<SendHarness ptySessionId="session-listen" />);
    const hasSendListener = () =>
      vi.mocked(listen).mock.calls.some((args) => args[0] === TREQ_SEND_EVENT);
    await waitFor(() => {
      expect(hasSendListener()).toBe(true);
    });
  });

  it("sends a toolbar review comment to the agent on Enter", async () => {
    const user = userEvent.setup();
    const onSendReview = vi.fn();
    renderSend(
      <SendHarness ptySessionId="session-1" onSendReview={onSendReview} />,
    );
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-1"));
    const input = await screen.findByTestId("treq-send-review-input");
    await user.type(input, "Please tighten this copy{Enter}");
    expect(onSendReview).toHaveBeenCalledWith(
      expect.stringContaining("Please tighten this copy"),
    );
  });

  it("warns when closing the lightbox with unsent comments", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" onSendReview={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-1"));
    await user.type(
      await screen.findByTestId("treq-send-review-input"),
      "unsent",
    );
    await user.click(screen.getByTestId("treq-send-close"));
    expect(await screen.findByTestId("treq-send-unsaved-dialog")).toBeTruthy();
    expect(screen.getByTestId("treq-send-preview-lightbox")).toBeTruthy();
    await user.click(screen.getByTestId("treq-send-unsaved-discard"));
    await waitFor(() => {
      expect(screen.queryByTestId("treq-send-preview-lightbox")).toBeNull();
    });
  });

  it("adds an inline line comment on a text asset", async () => {
    const user = userEvent.setup();
    const onSendReview = vi.fn();
    renderSend(
      <SendHarness ptySessionId="session-1" onSendReview={onSendReview} />,
    );
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-1"));
    await waitFor(() => {
      expect(screen.getByTestId("treq-send-text-line-1")).toBeTruthy();
    });
    await user.click(screen.getByTestId("treq-send-text-line-1"));
    const composer = await screen.findByTestId(
      "treq-send-line-comment-composer",
    );
    await user.type(
      composer.querySelector("textarea") as HTMLTextAreaElement,
      "Rename these",
    );
    await user.click(screen.getByTestId("treq-send-line-comment-save"));
    await user.type(screen.getByTestId("treq-send-review-input"), "{Enter}");
    expect(onSendReview).toHaveBeenCalledWith(
      expect.stringContaining("Rename these"),
    );
    expect(onSendReview).toHaveBeenCalledWith(
      expect.stringContaining("note.txt:1"),
    );
  });

  it("draws a red highlight on an image and includes the comment in the prompt", async () => {
    const user = userEvent.setup();
    const onSendReview = vi.fn();
    vi.spyOn(sendAssetReview, "renderHighlightedImageBlob").mockRejectedValue(
      new Error("skip canvas"),
    );
    if (!Element.prototype.setPointerCapture) {
      Element.prototype.setPointerCapture = () => {};
      Element.prototype.releasePointerCapture = () => {};
    }
    renderSend(
      <SendHarness ptySessionId="session-1" onSendReview={onSendReview} />,
    );
    await user.click(screen.getByRole("button", { name: "Inject image" }));
    await user.click(await screen.findByTestId("terminal-send-preview-send-2"));
    const surface = await screen.findByTestId("treq-send-highlight-surface");
    vi.spyOn(surface, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      bottom: 100,
      right: 100,
      width: 100,
      height: 100,
      toJSON: () => ({}),
    });
    await user.pointer([
      { keys: "[MouseLeft>]", target: surface, coords: { x: 10, y: 10 } },
      { target: surface, coords: { x: 50, y: 40 } },
      { keys: "[/MouseLeft]", target: surface, coords: { x: 50, y: 40 } },
    ]);
    const commentBox = await screen.findByLabelText("Highlight comment");
    await user.type(commentBox, "Move this button");
    await user.type(
      screen.getByTestId("treq-send-review-input"),
      "Header is too loud{Enter}",
    );
    expect(onSendReview).toHaveBeenCalledWith(
      expect.stringContaining("Move this button"),
    );
    expect(onSendReview).toHaveBeenCalledWith(
      expect.stringContaining("Header is too loud"),
    );
    expect(onSendReview).toHaveBeenCalledWith(
      expect.stringContaining("Highlights:"),
    );
  });

  it("drags a thumbnail with the quoted asset path and inserts it on drop", async () => {
    const user = userEvent.setup();
    const onInsertIntoTerminal = vi.fn();
    renderSend(
      <SendHarness
        ptySessionId="session-1"
        onInsertIntoTerminal={onInsertIntoTerminal}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    const thumb = await screen.findByTestId("terminal-send-preview-send-1");
    expect(thumb.getAttribute("draggable")).toBe("true");

    const store = new Map<string, string>();
    const types: string[] = [];
    const dataTransfer = {
      dropEffect: "none",
      effectAllowed: "all",
      files: [] as unknown as FileList,
      items: [] as unknown as DataTransferItemList,
      types,
      clearData: () => store.clear(),
      getData: (format: string) => store.get(format) ?? "",
      setData: (format: string, data: string) => {
        store.set(format, data);
        if (!types.includes(format)) types.push(format);
      },
      setDragImage: () => {},
    } as DataTransfer;

    fireEvent.dragStart(thumb, { dataTransfer });
    expect(dataTransfer.getData(SEND_ASSET_MIME)).toContain(
      "/tmp/repo/.treq/send/note.txt",
    );
    expect(dataTransfer.getData("text/plain")).toBe(
      "'/tmp/repo/.treq/send/note.txt'",
    );

    fireEvent.drop(screen.getByTestId("terminal-send-previews-drop"), {
      dataTransfer,
    });
    expect(onInsertIntoTerminal).toHaveBeenCalledWith(
      "'/tmp/repo/.treq/send/note.txt'",
    );
  });

  it("copies the quoted asset path when the thumbnail is hovered and the user copies", async () => {
    const user = userEvent.setup();
    renderSend(<SendHarness ptySessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: "Inject text" }));
    const thumb = await screen.findByTestId("terminal-send-preview-send-1");
    const attachment = thumb.closest('[data-slot="attachment"]') as HTMLElement;
    await user.hover(attachment);
    await user.keyboard("{Meta>}c{/Meta}");
    expect(sendAssetDrag.copySendAssetToClipboard).toHaveBeenCalledWith({
      path: "/tmp/repo/.treq/send/note.txt",
      title: "note.txt",
    });

    const clipboardData = {
      dropEffect: "none",
      effectAllowed: "all",
      files: [] as unknown as FileList,
      items: [] as unknown as DataTransferItemList,
      types: [] as string[],
      clearData: () => {},
      getData: () => "",
      setData: vi.fn(),
      setDragImage: () => {},
    } as unknown as DataTransfer;
    fireEvent.copy(attachment, { clipboardData });
    expect(clipboardData.setData).toHaveBeenCalled();
  });
});
