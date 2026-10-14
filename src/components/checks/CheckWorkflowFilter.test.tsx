import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { WorkflowInfo } from "../../lib/api-types";
import { CheckWorkflowFilter } from "./CheckWorkflowFilter";

const workflows = [
  { filename: "ci.yaml", name: "CI" },
  { filename: "docs.yaml", name: "Docs" },
  { filename: "lint.yaml", name: "Lint" },
] as WorkflowInfo[];

function setup(value: string[]) {
  const onChange = vi.fn();
  render(
    <CheckWorkflowFilter
      workflows={workflows}
      value={value}
      onChange={onChange}
    />,
  );
  return {
    onChange,
    trigger: screen.getByRole("button", { name: "Filter by check" }),
  };
}

describe("CheckWorkflowFilter", () => {
  it("labels an empty selection as all checks", () => {
    expect(setup([]).trigger.textContent).toContain("All checks");
  });

  it("labels one selection with the workflow name", () => {
    expect(setup(["docs.yaml"]).trigger.textContent).toContain("Docs");
  });

  it("labels several selections with a count", () => {
    expect(setup(["ci.yaml", "lint.yaml"]).trigger.textContent).toContain(
      "2 checks",
    );
  });

  it("adds and removes workflows from the selection", async () => {
    const user = userEvent.setup();
    const { onChange, trigger } = setup(["ci.yaml"]);
    await user.click(trigger);
    const ci = await screen.findByRole("menuitemcheckbox", { name: /CI/ });
    expect(ci.getAttribute("aria-checked")).toBe("true");
    expect(screen.getAllByRole("menuitemcheckbox")).toHaveLength(3);

    await user.click(screen.getByRole("menuitemcheckbox", { name: "Lint" }));
    expect(onChange).toHaveBeenLastCalledWith(["ci.yaml", "lint.yaml"]);
    await user.click(ci);
    expect(onChange).toHaveBeenLastCalledWith([]);
  });
});
