import { cn } from "../lib/utils";

/** Single-line text that truncates with an ellipsis at the end. */
export function TruncatedText({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  return (
    <span title={text} className={cn("block truncate", className)}>
      {text}
    </span>
  );
}
