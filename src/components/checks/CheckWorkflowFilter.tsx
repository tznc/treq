import type { WorkflowInfo } from "../../lib/api-types";

/** Picks which workflow's check logs to show. An empty value means all. */
export function CheckWorkflowFilter({
  workflows,
  value,
  onChange,
}: {
  workflows: WorkflowInfo[];
  value: string;
  onChange: (filename: string) => void;
}) {
  return (
    <select
      aria-label="Filter by check"
      data-testid="check-workflow-filter"
      className="h-8 rounded-md border bg-background px-2 text-sm min-w-[130px]"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    >
      <option value="">All checks</option>
      {workflows.map((wf) => (
        <option key={wf.filename} value={wf.filename}>
          {wf.name}
        </option>
      ))}
    </select>
  );
}
