import { ContextMenu as ContextMenuPrimitive } from "@base-ui/react/context-menu";
import { Menu as MenuPrimitive } from "@base-ui/react/menu";
import * as React from "react";
import { cn } from "../../lib/utils";

const ContextMenu = ContextMenuPrimitive.Root;

const ContextMenuTrigger = ({
  asChild,
  children,
  ...props
}: React.ComponentProps<typeof ContextMenuPrimitive.Trigger> & {
  asChild?: boolean;
}) =>
  asChild && React.isValidElement(children) ? (
    <ContextMenuPrimitive.Trigger {...props} render={children} />
  ) : (
    <ContextMenuPrimitive.Trigger {...props}>
      {children}
    </ContextMenuPrimitive.Trigger>
  );
ContextMenuTrigger.displayName = "ContextMenuTrigger";

const ContextMenuSub = MenuPrimitive.SubmenuRoot;

const ContextMenuSubTrigger = ({
  className,
  inset,
  children,
  ref,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.SubmenuTrigger> & {
  inset?: boolean;
}) => (
  <MenuPrimitive.SubmenuTrigger
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
  </MenuPrimitive.SubmenuTrigger>
);
ContextMenuSubTrigger.displayName = "ContextMenuSubTrigger";

const ContextMenuSubContent = ({
  className,
  ref,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.Popup>) => (
  <MenuPrimitive.Positioner className="z-50">
    <MenuPrimitive.Popup
      ref={ref}
      className={cn(
        "z-50 min-w-[8rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-lg data-[open]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[open]:fade-in-0 data-[closed]:zoom-out-95 data-[open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
        className,
      )}
      {...props}
    />
  </MenuPrimitive.Positioner>
);
ContextMenuSubContent.displayName = "ContextMenuSubContent";

const ContextMenuContent = ({
  className,
  ref,
  ...props
}: React.ComponentProps<typeof ContextMenuPrimitive.Popup>) => (
  <ContextMenuPrimitive.Portal>
    <ContextMenuPrimitive.Positioner className="z-50">
      <ContextMenuPrimitive.Popup
        ref={ref}
        className={cn(
          "z-50 min-w-[8rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-md",
          "data-[open]:animate-in data-[closed]:animate-out data-[closed]:fade-out-0 data-[open]:fade-in-0 data-[closed]:zoom-out-95 data-[open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
          className,
        )}
        {...props}
      />
    </ContextMenuPrimitive.Positioner>
  </ContextMenuPrimitive.Portal>
);
ContextMenuContent.displayName = "ContextMenuContent";

const ContextMenuItem = ({
  className,
  inset,
  onSelect,
  onClick,
  ref,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.Item> & {
  inset?: boolean;
  /** @deprecated Base UI's Menu.Item has no `onSelect`; kept for API parity with the previous Radix-based component. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}) => (
  <MenuPrimitive.Item
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
ContextMenuItem.displayName = "ContextMenuItem";

const ContextMenuCheckboxItem = ({
  className,
  children,
  checked,
  onSelect,
  onClick,
  ref,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.CheckboxItem> & {
  /** @deprecated Base UI's Menu.CheckboxItem has no `onSelect`; kept for API parity with the previous Radix-based component. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}) => (
  <MenuPrimitive.CheckboxItem
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
      <MenuPrimitive.CheckboxItemIndicator>
        <span className="h-4 w-4">✓</span>
      </MenuPrimitive.CheckboxItemIndicator>
    </span>
    {children}
  </MenuPrimitive.CheckboxItem>
);
ContextMenuCheckboxItem.displayName = "ContextMenuCheckboxItem";

const ContextMenuRadioItem = ({
  className,
  children,
  onSelect,
  onClick,
  ref,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.RadioItem> & {
  /** @deprecated Base UI's Menu.RadioItem has no `onSelect`; kept for API parity with the previous Radix-based component. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}) => (
  <MenuPrimitive.RadioItem
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
      <MenuPrimitive.RadioItemIndicator>
        <span className="h-2 w-2 rounded-full bg-current" />
      </MenuPrimitive.RadioItemIndicator>
    </span>
    {children}
  </MenuPrimitive.RadioItem>
);
ContextMenuRadioItem.displayName = "ContextMenuRadioItem";

const ContextMenuLabel = ({
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
ContextMenuLabel.displayName = "ContextMenuLabel";

const ContextMenuSeparator = ({
  className,
  ref,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.Separator>) => (
  <MenuPrimitive.Separator
    ref={ref}
    className={cn("-mx-1 my-1 h-px bg-muted", className)}
    {...props}
  />
);
ContextMenuSeparator.displayName = "ContextMenuSeparator";

const ContextMenuShortcut = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLSpanElement>) => (
  <span
    className={cn("ml-auto text-sm tracking-widest opacity-60", className)}
    {...props}
  />
);
ContextMenuShortcut.displayName = "ContextMenuShortcut";

export {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
};
