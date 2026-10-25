import type * as React from "react";
import { type VariantProps, cva } from "class-variance-authority";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-40",
  {
    variants: {
      size: {
        default: "h-10 px-4 py-2 text-base",
        icon: "h-10 w-10",
        "icon-xs": "h-6 w-6",
        lg: "h-11 px-6 text-lg",
        sm: "h-8 px-3 text-sm",
        xs: "h-6 px-2 text-sm",
      },
      variant: {
        default:
          "bg-primary text-primary-foreground hover:bg-primary/85 active:bg-primary/75 shadow-sm",
        destructive:
          "bg-destructive text-destructive-foreground hover:bg-destructive/85 active:bg-destructive/75 shadow-sm",
        ghost:
          "hover:bg-accent hover:text-accent-foreground active:bg-accent/80",
        link: "text-primary underline-offset-4 hover:underline",
        outline:
          "border border-input bg-background hover:bg-accent hover:text-accent-foreground active:bg-accent/80",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-secondary/70 active:bg-secondary/60",
      },
    },
    defaultVariants: {
      size: "default",
      variant: "default",
    },
  },
);

export interface ButtonProps
  extends React.ComponentProps<"button">,
    VariantProps<typeof buttonVariants> {}

const Button = ({ className, variant, size, ref, ...props }: ButtonProps) => (
  <button
    className={cn(buttonVariants({ className, size, variant }))}
    ref={ref}
    {...props}
  />
);
Button.displayName = "Button";

export { Button, buttonVariants };
