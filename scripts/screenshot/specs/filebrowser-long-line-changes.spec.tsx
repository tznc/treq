import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  commitRepoFile,
  createTestRepo,
  openRepo,
  writeRepoFile,
} from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

const OLD_LINE =
  "export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(({className, ...props}, ref) => <input ref={ref} className={cn('flex h-9 w-full rounded-md border border-zinc-200 bg-white px-3 py-1 text-sm outline-none', className)} {...props} />);";

const NEW_LINE =
  "export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(({className, ...props}, ref) => <input ref={ref} className={cn('flex h-9 w-full rounded-md border border-zinc-200 bg-white px-3 py-1 text-sm outline-none focus:ring-2 focus:ring-zinc-300', className)} {...props} />);";

it("captures FileBrowser change highlighting on a very long modified line", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  await commitRepoFile(
    repoPath,
    "input.tsx",
    [
      "import * as React from 'react';",
      "import {cn} from '../../lib/utils';",
      OLD_LINE,
      "Input.displayName = 'Input';",
    ].join("\n"),
    "add Input",
  );
  await writeRepoFile(
    repoPath,
    "input.tsx",
    [
      "import * as React from 'react';",
      "import {cn} from '../../lib/utils';",
      NEW_LINE,
      "Input.displayName = 'Input';",
      "",
    ].join("\n"),
    false,
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await user.click(await screen.findByRole("button", { name: "input.tsx" }));
  const fileBrowser = within(await screen.findByTestId("file-browser"));
  await fileBrowser.findByText(/forwardRef/);

  const longRow = (await fileBrowser.findAllByTestId("code-line")).find(
    (line) => (line.textContent ?? "").includes("forwardRef"),
  );
  expect(longRow).toBeTruthy();
  expect(longRow).toHaveClass("overflow-hidden");
  expect((longRow as HTMLElement).style.minHeight).toBe("18px");

  await captureDocument(document, {
    name: "filebrowser-long-line-changes-01-wrapped",
    expectations: [
      "The long Input.forwardRef line wraps onto multiple visual lines with no horizontal scrollbar.",
      "Input.displayName sits below the wrapped text and is not overlapped by it.",
      "Deleted and added text are not stacked on top of each other in the same row.",
    ],
  });
}, 60000);
