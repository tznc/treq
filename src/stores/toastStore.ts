import { create } from "zustand";

export type ToastType = "success" | "error" | "info" | "warning";

export interface Toast {
  id: string;
  title: string;
  description?: string;
  type: ToastType;
  action?: {
    label: string;
    onClick: () => void;
  };
}

/** A toast as the GPUI shell draws it: its action reduced to a label. */
export interface GpuiToast {
  id: string;
  title: string;
  description?: string;
  type: ToastType;
  action?: string;
}

export function toGpuiToasts(toasts: Toast[]): GpuiToast[] {
  return toasts.map(({ action, ...toast }) => ({
    ...toast,
    ...(action ? { action: action.label } : {}),
  }));
}

const toastTimers = new Map<string, ReturnType<typeof setTimeout>>();

export interface ToastState {
  toasts: Toast[];
  addToast: (toast: Omit<Toast, "id">) => string;
  removeToast: (id: string) => void;
}

export const defaultToastState = {
  toasts: [] as Toast[],
};

export function clearToastTimers() {
  for (const timer of toastTimers.values()) {
    clearTimeout(timer);
  }
  toastTimers.clear();
}

function expireToast(id: string) {
  toastTimers.delete(id);
  useToastStore.setState((state) => ({
    toasts: state.toasts.filter((item) => item.id !== id),
  }));
}

export const useToastStore = create<ToastState>((set) => ({
  ...defaultToastState,
  addToast: (toast) => {
    const id = Math.random().toString(36).substring(7);
    set((state) => ({ toasts: [...state.toasts, { ...toast, id }] }));

    const durationMs = toast.action ? 12000 : 5000;
    const timer = setTimeout(() => expireToast(id), durationMs);
    toastTimers.set(id, timer);
    return id;
  },
  removeToast: (id) => {
    const timer = toastTimers.get(id);
    if (timer) {
      clearTimeout(timer);
      toastTimers.delete(id);
    }
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    }));
  },
}));
