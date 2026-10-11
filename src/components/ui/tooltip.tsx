import { Tooltip as TooltipPrimitive } from "@base-ui/react/tooltip";
import * as React from "react";
import { cn } from "../../lib/utils";

const TooltipProvider = TooltipPrimitive.Provider;
const Tooltip = TooltipPrimitive.Root;

const TooltipTrigger = ({
  asChild,
  children,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Trigger> & {
  asChild?: boolean;
}) =>
  asChild && React.isValidElement(children) ? (
    <TooltipPrimitive.Trigger {...props} render={children} />
  ) : (
    <TooltipPrimitive.Trigger {...props}>{children}</TooltipPrimitive.Trigger>
  );
TooltipTrigger.displayName = "TooltipTrigger";

const TooltipPortal = TooltipPrimitive.Portal;
const TooltipArrow = TooltipPrimitive.Arrow;

const TooltipContent = ({
  className,
  align = "center",
  alignOffset,
  side,
  sideOffset = 6,
  ref,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Popup> & {
  align?: "start" | "center" | "end";
  alignOffset?: number;
  side?: "top" | "bottom" | "left" | "right";
  sideOffset?: number;
}) => (
  <TooltipPortal>
    {/* The positioner's transform makes it the stacking context, so it needs
        the z-index for the tooltip to show above z-indexed page content. */}
    <TooltipPrimitive.Positioner
      align={align}
      alignOffset={alignOffset}
      side={side}
      sideOffset={sideOffset}
      className="z-50"
    >
      <TooltipPrimitive.Popup
        ref={ref}
        role="tooltip"
        className={cn(
          "z-50 overflow-hidden rounded-md border bg-popover px-3 py-1.5 text-sm text-popover-foreground shadow-md data-[closed]:animate-out data-[open]:animate-in data-[closed]:fade-out-0 data-[open]:fade-in-0 data-[closed]:zoom-out-95 data-[open]:zoom-in-95",
          "data-[side=bottom]:slide-in-from-top-1 data-[side=top]:slide-in-from-bottom-1 data-[side=left]:slide-in-from-right-1 data-[side=right]:slide-in-from-left-1",
          className,
        )}
        {...props}
      />
    </TooltipPrimitive.Positioner>
  </TooltipPortal>
);
TooltipContent.displayName = "TooltipContent";

export {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
  TooltipProvider,
  TooltipPortal,
  TooltipArrow,
};
