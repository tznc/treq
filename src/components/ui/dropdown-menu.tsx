import { Menu as DropdownMenuPrimitive } from "@base-ui/react/menu";
import * as React from "react";
import { cn } from "../../lib/utils";

const DropdownMenu = DropdownMenuPrimitive.Root;

const DropdownMenuTrigger = ({
  asChild,
  children,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.Trigger> & {
  asChild?: boolean;
}) =>
  asChild && React.isValidElement(children) ? (
    <DropdownMenuPrimitive.Trigger {...props} render={children} />
  ) : (
    <DropdownMenuPrimitive.Trigger {...props}>
      {children}
    </DropdownMenuPrimitive.Trigger>
  );
DropdownMenuTrigger.displayName = "DropdownMenuTrigger";

const DropdownMenuSub = DropdownMenuPrimitive.SubmenuRoot;

const DropdownMenuRadioGroup = DropdownMenuPrimitive.RadioGroup;

const DropdownMenuSubTrigger = ({
  className,
  inset,
  children,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.SubmenuTrigger> & {
  inset?: boolean;
}) => (
  <DropdownMenuPrimitive.SubmenuTrigger
    ref={ref}
    className={cn(
      "flex cursor-pointer select-none items-center rounded-sm px-2 py-1.5 text-sm outline-none focus:bg-accent data-[popup-open]:bg-accent",
      inset && "pl-8",
      className,
    )}
    {...props}
  >
    {children}
    <span className="ml-auto">›</span>
  </DropdownMenuPrimitive.SubmenuTrigger>
);
DropdownMenuSubTrigger.displayName = "DropdownMenuSubTrigger";

const DropdownMenuSubContent = ({
  className,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.Popup>) => (
  <DropdownMenuPrimitive.Positioner>
    <DropdownMenuPrimitive.Popup
      ref={ref}
      className={cn(
        "z-50 min-w-[8rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-lg data-[open]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[open]:fade-in-0 data-[closed]:zoom-out-95 data-[open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
        className,
      )}
      {...props}
    />
  </DropdownMenuPrimitive.Positioner>
);
DropdownMenuSubContent.displayName = "DropdownMenuSubContent";

const DropdownMenuContent = ({
  className,
  align,
  alignOffset,
  side,
  sideOffset = 4,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.Popup> & {
  align?: "start" | "center" | "end";
  alignOffset?: number;
  side?: "top" | "bottom" | "left" | "right";
  sideOffset?: number;
}) => (
  <DropdownMenuPrimitive.Portal>
    {/* The positioner's transform makes it the stacking context, so it needs
        the z-index for the menu to open above dialogs. */}
    <DropdownMenuPrimitive.Positioner
      align={align}
      alignOffset={alignOffset}
      side={side}
      sideOffset={sideOffset}
      className="z-50"
    >
      <DropdownMenuPrimitive.Popup
        ref={ref}
        className={cn(
          "z-50 min-w-[8rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-md",
          "data-[open]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[open]:fade-in-0 data-[closed]:zoom-out-95 data-[open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
          className,
        )}
        {...props}
      />
    </DropdownMenuPrimitive.Positioner>
  </DropdownMenuPrimitive.Portal>
);
DropdownMenuContent.displayName = "DropdownMenuContent";

const DropdownMenuItem = ({
  className,
  inset,
  onSelect,
  onClick,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.Item> & {
  inset?: boolean;
  /** @deprecated Base UI's Menu.Item has no `onSelect`; kept for API parity with the previous Radix-based component. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}) => (
  <DropdownMenuPrimitive.Item
    ref={ref}
    onClick={(event) => {
      onClick?.(event);
      onSelect?.(event);
    }}
    className={cn(
      "relative flex cursor-pointer select-none items-center rounded-sm px-2 py-1.5 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
      inset && "pl-8",
      className,
    )}
    {...props}
  />
);
DropdownMenuItem.displayName = "DropdownMenuItem";

const DropdownMenuCheckboxItem = ({
  className,
  children,
  checked,
  onSelect,
  onClick,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.CheckboxItem> & {
  /** @deprecated Base UI's Menu.CheckboxItem has no `onSelect`; kept for API parity with the previous Radix-based component. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}) => (
  <DropdownMenuPrimitive.CheckboxItem
    ref={ref}
    onClick={(event) => {
      onClick?.(event);
      onSelect?.(event);
    }}
    className={cn(
      "relative flex cursor-pointer select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
      className,
    )}
    checked={checked}
    {...props}
  >
    <span className="absolute left-2 flex h-4 w-4 items-center justify-center">
      <DropdownMenuPrimitive.CheckboxItemIndicator>
        <span className="h-4 w-4">✓</span>
      </DropdownMenuPrimitive.CheckboxItemIndicator>
    </span>
    {children}
  </DropdownMenuPrimitive.CheckboxItem>
);
DropdownMenuCheckboxItem.displayName = "DropdownMenuCheckboxItem";

const DropdownMenuRadioItem = ({
  className,
  children,
  onSelect,
  onClick,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.RadioItem> & {
  /** @deprecated Base UI's Menu.RadioItem has no `onSelect`; kept for API parity with the previous Radix-based component. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}) => (
  <DropdownMenuPrimitive.RadioItem
    ref={ref}
    onClick={(event) => {
      onClick?.(event);
      onSelect?.(event);
    }}
    className={cn(
      "relative flex cursor-pointer select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
      className,
    )}
    {...props}
  >
    <span className="absolute left-2 flex h-4 w-4 items-center justify-center">
      <DropdownMenuPrimitive.RadioItemIndicator>
        <span className="block h-2 w-2 rounded-full bg-current" />
      </DropdownMenuPrimitive.RadioItemIndicator>
    </span>
    {children}
  </DropdownMenuPrimitive.RadioItem>
);
DropdownMenuRadioItem.displayName = "DropdownMenuRadioItem";

const DropdownMenuLabel = ({
  className,
  inset,
  ref,
  ...props
}: React.ComponentProps<"div"> & {
  inset?: boolean;
}) => (
  <div
    ref={ref}
    className={cn(
      "px-2 py-1.5 text-sm font-semibold",
      inset && "pl-8",
      className,
    )}
    {...props}
  />
);
DropdownMenuLabel.displayName = "DropdownMenuLabel";

const DropdownMenuSeparator = ({
  className,
  ref,
  ...props
}: React.ComponentProps<typeof DropdownMenuPrimitive.Separator>) => (
  <DropdownMenuPrimitive.Separator
    ref={ref}
    className={cn("-mx-1 my-1 h-px bg-muted", className)}
    {...props}
  />
);
DropdownMenuSeparator.displayName = "DropdownMenuSeparator";

const DropdownMenuShortcut = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLSpanElement>) => (
  <span
    className={cn("ml-auto text-sm tracking-widest opacity-60", className)}
    {...props}
  />
);
DropdownMenuShortcut.displayName = "DropdownMenuShortcut";

export {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
  DropdownMenuRadioItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuRadioGroup,
};
