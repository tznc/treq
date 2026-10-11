import * as React from "react";
import { cn } from "../../lib/utils";

type InputProps = React.ComponentProps<"input">;

const Input = ({ className, type, ref, ...props }: InputProps) => (
  <input
    type={type}
    className={cn(
      "flex h-10 w-full rounded-sm border border-input bg-background px-3 py-2 text-sm transition-colors ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-40",
      className,
    )}
    autoComplete="off"
    autoCorrect="off"
    autoCapitalize="off"
    spellCheck={false}
    ref={ref}
    {...props}
  />
);
Input.displayName = "Input";

export { Input };
