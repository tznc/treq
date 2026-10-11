import { Popover as PopoverPrimitive } from "@base-ui/react/popover";
import * as React from "react";
import { cn } from "../../lib/utils";

const Popover = PopoverPrimitive.Root;

const PopoverTrigger = ({
  asChild,
  children,
  ...props
}: React.ComponentProps<typeof PopoverPrimitive.Trigger> & {
  asChild?: boolean;
}) =>
  asChild && React.isValidElement(children) ? (
    <PopoverPrimitive.Trigger {...props} render={children} />
  ) : (
    <PopoverPrimitive.Trigger {...props}>{children}</PopoverPrimitive.Trigger>
  );
PopoverTrigger.displayName = "PopoverTrigger";

const PopoverContent = ({
  className,
  align = "center",
  alignOffset,
  side,
  sideOffset = 4,
  ref,
  ...props
}: React.ComponentProps<typeof PopoverPrimitive.Popup> & {
  align?: "start" | "center" | "end";
  alignOffset?: number;
  side?: "top" | "bottom" | "left" | "right";
  sideOffset?: number;
}) => (
  <PopoverPrimitive.Portal>
    {/* The positioner's transform makes it the stacking context, so it needs
        the z-index for the popup to open above z-indexed page content. */}
    <PopoverPrimitive.Positioner
      align={align}
      alignOffset={alignOffset}
      side={side}
      sideOffset={sideOffset}
      className="z-50"
    >
      <PopoverPrimitive.Popup
        ref={ref}
        className={cn(
          "z-50 w-72 rounded-md border bg-popover p-4 text-popover-foreground shadow-md outline-none data-[open]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[open]:fade-in-0 data-[closed]:zoom-out-95 data-[open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Positioner>
  </PopoverPrimitive.Portal>
);
PopoverContent.displayName = "PopoverContent";

export { Popover, PopoverTrigger, PopoverContent };
