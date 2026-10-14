import type { WorkflowInfo } from "../../lib/api-types";
import { MultiSelectFilter } from "../MultiSelectFilter";

/** Picks which workflows' check logs to show. Nothing ticked means all. */
export function CheckWorkflowFilter({
  workflows,
  value,
  onChange,
}: {
  workflows: WorkflowInfo[];
  /** Selected workflow filenames. */
  value: string[];
  onChange: (filenames: string[]) => void;
}) {
  return (
    <MultiSelectFilter
      options={workflows.map((wf) => ({ value: wf.filename, label: wf.name }))}
      value={value}
      onChange={onChange}
      allLabel="All checks"
      pluralNoun="checks"
      ariaLabel="Filter by check"
      testId="check-workflow-filter"
      contentClassName="w-48"
    />
  );
}
