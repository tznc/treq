import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import * as api from "../../lib/api";
import { ToastProvider } from "../ui/toast";
import { SendAssetLightbox } from "./SendAssetLightbox";

vi.spyOn(api, "readFile").mockResolvedValue("hello from lightbox");

describe("SendAssetLightbox", () => {
  it("closes when clicking empty space around the file, not the file", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <ToastProvider>
        <SendAssetLightbox
          assets={[
            {
              id: "a1",
              repo: "/tmp/repo",
              ptySessionId: null,
              mediaType: "text",
              path: "/tmp/repo/.treq/send/backdrop.txt",
              title: "backdrop.txt",
              receivedAt: 0,
            },
          ]}
          initialIndex={0}
          onClose={onClose}
        />
      </ToastProvider>,
    );
    await waitFor(() => {
      expect(
        screen.getByTestId("treq-send-text-preview").textContent,
      ).toContain("hello from lightbox");
    });
    await user.click(screen.getByTestId("treq-send-text-preview"));
    expect(onClose).not.toHaveBeenCalled();
    await user.click(screen.getByTestId("treq-send-preview-carousel-shell"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
