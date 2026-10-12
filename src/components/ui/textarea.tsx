import * as React from "react";
import { cn } from "../../lib/utils";

type TextareaProps = React.ComponentProps<"textarea"> & {
  /** Grow and shrink the height to fit the content instead of a fixed size. */
  autoResize?: boolean;
};

// Chromium sizes the textarea from CSS alone. WebKit (the macOS webview) has
// no field-sizing support, so it falls back to measuring scrollHeight.
const supportsFieldSizing =
  typeof CSS !== "undefined" &&
  typeof CSS.supports === "function" &&
  CSS.supports("field-sizing", "content");

const Textarea = ({
  className,
  ref,
  autoResize = false,
  value,
  ...props
}: TextareaProps) => {
  const innerRef = React.useRef<HTMLTextAreaElement | null>(null);

  const setRefs = (node: HTMLTextAreaElement | null) => {
    innerRef.current = node;
    if (typeof ref === "function") ref(node);
    else if (ref) ref.current = node;
  };

  React.useLayoutEffect(() => {
    const textarea = innerRef.current;
    if (!autoResize || supportsFieldSizing || !textarea) return;
    // Reset first so the height can shrink when text is removed.
    textarea.style.height = "auto";
    // A hidden or unlaid-out element reports 0; keep the rows-based height.
    if (textarea.scrollHeight === 0) return;
    // scrollHeight excludes borders, and the element is border-box.
    const borders = textarea.offsetHeight - textarea.clientHeight;
    textarea.style.height = `${textarea.scrollHeight + borders}px`;
  }, [autoResize, value]);

  return (
    <textarea
      className={cn(
        "flex min-h-[80px] w-full rounded-sm border border-input bg-background px-3 py-2 transition-colors ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-40 resize-none",
        autoResize && "min-h-0 overflow-hidden [field-sizing:content]",
        className,
      )}
      autoComplete="off"
      autoCorrect="off"
      autoCapitalize="off"
      spellCheck={false}
      ref={setRefs}
      value={value}
      {...props}
    />
  );
};
Textarea.displayName = "Textarea";

export { Textarea };
