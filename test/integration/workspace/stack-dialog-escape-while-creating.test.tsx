// @include-parallel
import { findGpuiRow, stackGpui } from "../../gpui-sidebar";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import * as api from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import { createTestRepo, openRepo } from "../../utils";

let gate: Promise<void> | null = null;
vi.mock("../../../src/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../src/lib/api")>();
  return {
    ...actual,
    createWorkspace: async (
      ...args: Parameters<typeof actual.createWorkspace>
    ) => {
      if (gate) await gate;
      return actual.createWorkspace(...args);
    },
  };
});

let user: ReturnType<typeof userEvent.setup>;
beforeEach(() => {
  user = userEvent.setup();
});

afterEach(() => {
  gate = null;
});

it("ignores Escape while a stacked workspace is being created", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await api.createWorkspace(repoPath, "esc/base");

  render(<Dashboard />);

  await stackGpui("esc/base");
  const dialog = await screen.findByTestId("modal");
  await user.type(
    await within(dialog).findByLabelText("Branch Name"),
    "esc/child",
  );

  let release!: () => void;
  gate = new Promise((resolve) => {
    release = resolve;
  });
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await within(dialog).findByRole("button", { name: /Creating/ });

  await user.keyboard("{Escape}");
  expect(screen.getByTestId("modal")).toBeInTheDocument();

  release();
  await waitFor(() =>
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
  );
  await findGpuiRow("esc/child");
}, 30000);
