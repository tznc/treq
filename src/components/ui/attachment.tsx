import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "./slot";

import { cn } from "../../lib/utils";
import { Button } from "./button";

const attachmentVariants = cva(
  "group/attachment relative flex w-fit max-w-full min-w-0 shrink-0 flex-wrap rounded-xl border bg-card text-card-foreground transition-colors focus-within:ring-1 focus-within:ring-ring/50 has-[>a,>button]:hover:bg-muted/50 data-[state=error]:border-destructive/30 data-[state=idle]:border-dashed",
  {
    variants: {
      size: {
        default:
          "gap-2 text-sm has-data-[slot=attachment-content]:px-2.5 has-data-[slot=attachment-content]:py-2 has-data-[slot=attachment-media]:p-2",
        sm: "gap-2.5 text-xs has-data-[slot=attachment-content]:px-2 has-data-[slot=attachment-content]:py-1.5 has-data-[slot=attachment-media]:p-1.5",
        xs: "gap-1.5 rounded-lg text-xs has-data-[slot=attachment-content]:px-1.5 has-data-[slot=attachment-content]:py-1 has-data-[slot=attachment-media]:p-1",
      },
      orientation: {
        horizontal: "min-w-40 items-center",
        vertical: "w-24 flex-col has-data-[slot=attachment-content]:w-30",
      },
      variant: {
        default: "",
        thumbnail:
          "h-20 w-36 min-w-36 flex-col flex-nowrap items-stretch justify-stretch gap-0 overflow-visible rounded-md border-white/15 bg-[#2a2a2a] p-0 text-zinc-300 shadow-md transition hover:border-white/40 has-[>a,>button]:hover:bg-[#2a2a2a] has-data-[slot=attachment-content]:p-0 has-data-[slot=attachment-content]:w-auto has-data-[slot=attachment-media]:p-0 focus-within:ring-1 focus-within:ring-white/30",
      },
    },
    defaultVariants: {
      size: "default",
      orientation: "horizontal",
      variant: "default",
    },
  },
);

function Attachment({
  className,
  state = "done",
  size = "default",
  orientation = "horizontal",
  variant = "default",
  ...props
}: React.ComponentProps<"div"> &
  VariantProps<typeof attachmentVariants> & {
    state?: "idle" | "uploading" | "processing" | "error" | "done";
  }) {
  const isThumbnail = variant === "thumbnail";
  const resolvedSize = isThumbnail ? null : size;
  const resolvedOrientation = isThumbnail ? null : orientation;

  return (
    <div
      data-slot="attachment"
      data-state={state}
      data-size={resolvedSize ?? undefined}
      data-orientation={resolvedOrientation ?? undefined}
      data-variant={variant}
      className={cn(
        attachmentVariants({
          size: resolvedSize,
          orientation: resolvedOrientation,
          variant,
        }),
        className,
      )}
      {...props}
    />
  );
}

const attachmentMediaVariants = cva(
  "relative flex aspect-square w-10 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-muted text-foreground group-data-[orientation=vertical]/attachment:w-full group-data-[size=sm]/attachment:w-8 group-data-[size=xs]/attachment:w-7 group-data-[size=xs]/attachment:rounded-md group-data-[state=error]/attachment:bg-destructive/10 group-data-[state=error]/attachment:text-destructive group-data-[orientation=vertical]/attachment:*:data-[slot=spinner]:size-6! [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 group-data-[orientation=vertical]/attachment:[&_svg:not([class*='size-'])]:size-6 group-data-[size=xs]/attachment:[&_svg:not([class*='size-'])]:size-3.5 group-data-[variant=thumbnail]/attachment:aspect-auto group-data-[variant=thumbnail]/attachment:size-full group-data-[variant=thumbnail]/attachment:rounded-[calc(var(--radius-md)-1px)] group-data-[variant=thumbnail]/attachment:bg-transparent group-data-[variant=thumbnail]/attachment:text-zinc-300 group-data-[variant=thumbnail]/attachment:[&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        icon: "group-data-[variant=thumbnail]/attachment:flex-col group-data-[variant=thumbnail]/attachment:gap-1 group-data-[variant=thumbnail]/attachment:px-1",
        image:
          "opacity-60 group-data-[state=done]/attachment:opacity-100 group-data-[state=idle]/attachment:opacity-100 *:[img]:aspect-square *:[img]:size-full *:[img]:object-cover group-data-[variant=thumbnail]/attachment:opacity-100 group-data-[variant=thumbnail]/attachment:*:[img]:aspect-auto group-data-[variant=thumbnail]/attachment:*:[img]:object-contain group-data-[variant=thumbnail]/attachment:*:[img]:rounded-[calc(var(--radius-md)-1px)]",
      },
    },
    defaultVariants: {
      variant: "icon",
    },
  },
);

function AttachmentMedia({
  className,
  variant = "icon",
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof attachmentMediaVariants>) {
  return (
    <div
      data-slot="attachment-media"
      data-variant={variant}
      className={cn(attachmentMediaVariants({ variant }), className)}
      {...props}
    />
  );
}

function AttachmentActions({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="attachment-actions"
      className={cn(
        "relative z-20 flex shrink-0 items-center group-data-[orientation=vertical]/attachment:absolute group-data-[orientation=vertical]/attachment:top-3 group-data-[orientation=vertical]/attachment:right-3 group-data-[orientation=vertical]/attachment:gap-1 group-data-[variant=thumbnail]/attachment:absolute group-data-[variant=thumbnail]/attachment:-right-2 group-data-[variant=thumbnail]/attachment:-top-2",
        className,
      )}
      {...props}
    />
  );
}

function AttachmentAction({
  className,
  variant,
  size = "icon-xs",
  ...props
}: React.ComponentProps<typeof Button>) {
  return (
    <Button
      data-slot="attachment-action"
      variant={variant ?? "ghost"}
      size={size}
      className={cn(className)}
      {...props}
    />
  );
}

function AttachmentTrigger({
  className,
  asChild = false,
  type,
  ...props
}: React.ComponentProps<"button"> & {
  asChild?: boolean;
}) {
  const Comp = asChild ? Slot : "button";

  return (
    <Comp
      data-slot="attachment-trigger"
      type={asChild ? undefined : (type ?? "button")}
      className={cn("absolute inset-0 z-10 outline-none", className)}
      {...props}
    />
  );
}

function AttachmentGroup({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="attachment-group"
      className={cn(
        "flex min-w-0 scroll-fade-x snap-x snap-mandatory scroll-px-1 scrollbar-none gap-3 overflow-x-auto overscroll-x-contain py-1 *:data-[slot=attachment]:flex-none *:data-[slot=attachment]:snap-start",
        className,
      )}
      {...props}
    />
  );
}

export {
  Attachment,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentActions,
  AttachmentAction,
  AttachmentTrigger,
};
