import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  Info,
  X,
} from "lucide-react";
import { cn } from "../../lib/utils";
import {
  useToastStore,
  type Toast,
  type ToastType,
} from "../../stores/toastStore";

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const toasts = useToastStore((s) => s.toasts);
  const removeToast = useToastStore((s) => s.removeToast);

  return (
    <>
      {children}
      <div className="fixed bottom-4 left-4 z-[60] flex flex-col gap-2">
        {toasts.map((toast) => (
          <ToastItem
            key={toast.id}
            toast={toast}
            onClose={() => removeToast(toast.id)}
          />
        ))}
      </div>
    </>
  );
}

const toastIcons: Record<ToastType, typeof CheckCircle2> = {
  error: AlertCircle,
  info: Info,
  success: CheckCircle2,
  warning: AlertTriangle,
};

const toastIconStyles: Record<ToastType, string> = {
  error: "text-destructive",
  info: "text-primary",
  success: "text-green-500",
  warning: "text-yellow-600 dark:text-yellow-400",
};

function ToastItem({ toast, onClose }: { toast: Toast; onClose: () => void }) {
  const Icon = toastIcons[toast.type];

  return (
    <div
      role="status"
      className="pointer-events-auto flex w-full min-w-[320px] max-w-sm items-start gap-3 rounded-lg border border-border bg-popover p-4 text-popover-foreground shadow-lg animate-in slide-in-from-left"
    >
      <Icon
        className={cn("mt-0.5 h-4 w-4 shrink-0", toastIconStyles[toast.type])}
      />
      <div className="flex-1 space-y-1">
        <div className="text-sm font-semibold leading-none">{toast.title}</div>
        {toast.description && (
          <div className="text-sm text-muted-foreground">
            {toast.description}
          </div>
        )}
        {toast.action && (
          <button
            type="button"
            onClick={() => {
              toast.action?.onClick();
              onClose();
            }}
            className="mt-1 text-sm font-medium text-primary underline underline-offset-2 hover:no-underline"
          >
            {toast.action.label}
          </button>
        )}
      </div>
      <button
        onClick={onClose}
        className="shrink-0 rounded-md p-1 text-muted-foreground/70 transition-colors hover:bg-accent hover:text-foreground"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}

export function useToast() {
  const toasts = useToastStore((s) => s.toasts);
  const addToast = useToastStore((s) => s.addToast);
  const removeToast = useToastStore((s) => s.removeToast);
  return { toasts, addToast, removeToast };
}
