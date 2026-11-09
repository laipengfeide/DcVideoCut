import { reactive } from "vue";
import { listen } from "@tauri-apps/api/event";
import type { ProgressPayload } from "./api";

export const progress = reactive({
  job: "",
  percent: 0,
  message: "",
});

let started = false;

export async function ensureProgressListener(): Promise<void> {
  if (started) return;
  started = true;
  try {
    await listen<ProgressPayload>("job-progress", (event) => {
      progress.job = event.payload.job;
      progress.percent = event.payload.percent;
      progress.message = event.payload.message;
    });
  } catch {
    /* ignore */
  }
}

export function resetProgress(message = ""): void {
  progress.percent = 0;
  progress.message = message;
}
