import * as React from "react";
import { cn } from "../../lib/utils";

interface DialogProps {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: React.ReactNode;
}

const DialogTitleIdContext = React.createContext<string | undefined>(undefined);

const Dialog: React.FC<DialogProps> = ({ open, onOpenChange, children }) => {
  const titleId = React.useId();
  if (!open) return null;

  return (
    <DialogTitleIdContext.Provider value={titleId}>
      <div
        data-window-modal
        className="fixed inset-0 z-50 flex items-center justify-center p-4"
        style={{ left: "calc(-1 * var(--window-offset-left, 0px))" }}
      >
        <div
          data-window-modal-backdrop
          className="fixed inset-0 bg-black/40 backdrop-blur-sm transition-opacity"
          style={{ left: "calc(-1 * var(--window-offset-left, 0px))" }}
          onClick={() => onOpenChange?.(false)}
        />
        <div className="relative z-50">{children}</div>
      </div>
    </DialogTitleIdContext.Provider>
  );
};

const DialogContent = ({
  className,
  children,
  ref,
  ...props
}: React.ComponentProps<"div">) => {
  const titleId = React.useContext(DialogTitleIdContext);
  return (
    <div
      ref={ref}
      data-testid="modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      className={cn(
        "bg-popover text-popover-foreground p-6 shadow-2xl duration-200 rounded-xl border border-border/50 max-w-lg w-full",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
};
DialogContent.displayName = "DialogContent";

const DialogHeader = ({ className, ...props }: React.ComponentProps<"div">) => (
  <div
    className={cn(
      "flex flex-col space-y-1.5 text-center sm:text-left",
      className,
    )}
    {...props}
  />
);
DialogHeader.displayName = "DialogHeader";

const DialogTitle = ({
  className,
  id,
  ref,
  ...props
}: React.ComponentProps<"h2">) => {
  const titleId = React.useContext(DialogTitleIdContext);
  return (
    <h2
      ref={ref}
      id={id ?? titleId}
      className={cn(
        "text-lg font-semibold leading-none tracking-tight",
        className,
      )}
      {...props}
    />
  );
};
DialogTitle.displayName = "DialogTitle";

const DialogDescription = ({
  className,
  ref,
  ...props
}: React.ComponentProps<"p">) => (
  <p
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
);
DialogDescription.displayName = "DialogDescription";

export { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription };
