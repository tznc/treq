import { expect } from "vitest";
import { waitFor } from "@testing-library/react";
import { gpuiRows } from "../../test/gpui-sidebar";

/**
 * Waits until the GPUI sidebar draws (or stops drawing) the conflict
 * indicator on workspace `id`'s row.
 */
export async function waitForGpuiConflict(
  id: number,
  conflicted = true,
): Promise<void> {
  await waitFor(() =>
    expect(gpuiRows().find((r) => r.id === id)?.conflicted ?? false).toBe(
      conflicted,
    ),
  );
}
