import { CheckCircle2, CircleDot, Loader2, XCircle } from "lucide-react";
import { cn } from "../../lib/utils";

/** One icon per run or summary status: passed, failed, running, or not run. */
export function ChecksStatusIcon({
  status,
  className,
}: {
  status: string | undefined;
  className?: string;
}) {
  const base = cn("h-4 w-4 shrink-0", className);
  if (status === "passed") {
    return (
      <CheckCircle2
        data-testid="run-status-passed"
        className={cn(base, "text-green-500")}
      />
    );
  }
  if (status === "failed") {
    return (
      <XCircle
        data-testid="run-status-failed"
        className={cn(base, "text-red-500")}
      />
    );
  }
  if (status === "running" || status === "pending") {
    return (
      <Loader2
        data-testid="run-status-running"
        className={cn(base, "animate-spin text-muted-foreground")}
      />
    );
  }
  return (
    <CircleDot
      data-testid="run-status-none"
      className={cn(base, "text-muted-foreground")}
    />
  );
}
