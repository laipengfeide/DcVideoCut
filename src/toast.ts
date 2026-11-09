import { reactive } from "vue";

export type ToastType = "info" | "success" | "error";

export interface Toast {
  id: number;
  text: string;
  type: ToastType;
}

export const toasts = reactive<Toast[]>([]);

let seq = 0;

export function toast(text: string, type: ToastType = "info"): void {
  const id = ++seq;
  toasts.push({ id, text, type });
  window.setTimeout(() => {
    const index = toasts.findIndex((t) => t.id === id);
    if (index >= 0) toasts.splice(index, 1);
  }, type === "error" ? 6000 : 3200);
}

export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
