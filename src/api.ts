import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export interface SessionView {
  isGuest: boolean;
  username: string | null;
  maxVideoBytes: number;
  guestFrameLimitBytes: number;
  frameLimitBytes: number | null;
  maxCutPoints: number | null;
  ffmpegOk: boolean;
  ffmpegMessage: string;
  lastOutDir: string | null;
}

export interface VideoMeta {
  path: string;
  fileName: string;
  sizeBytes: number;
  durationMs: number;
  width: number;
  height: number;
  fps: number;
  videoCodec: string;
  audioCodec: string | null;
  bitrate: number;
}

export interface FrameItem {
  index: number;
  timeMs: number;
  file: string;
  thumb: string;
}

export interface FramesResult {
  jobId: string;
  video: VideoMeta;
  frames: FrameItem[];
  intervalMs: number;
}

export interface FramePick {
  file: string;
  index: number;
  timeMs: number;
}

export interface SegmentInput {
  startMs: number;
  endMs: number;
  name?: string | null;
}

export interface SaveResult {
  saved: string[];
  outDir: string;
}

export interface ExportResult {
  outputs: string[];
  outDir: string;
  mode: string;
}

export interface ProgressPayload {
  job: string;
  percent: number;
  message: string;
}

export const api = {
  bootstrap: () => invoke<SessionView>("app_bootstrap"),
  register: (username: string, password: string) =>
    invoke<SessionView>("auth_register", { username, password }),
  login: (username: string, password: string) =>
    invoke<SessionView>("auth_login", { username, password }),
  guest: () => invoke<SessionView>("auth_guest"),
  logout: () => invoke<SessionView>("auth_logout"),
  probe: (path: string) => invoke<VideoMeta>("probe_video", { path }),
  extract: (path: string, intervalMs: number) =>
    invoke<FramesResult>("extract_frames", { path, intervalMs }),
  timelineThumbs: (path: string, count: number) =>
    invoke<FrameItem[]>("timeline_thumbs", { path, count }),
  saveFrames: (
    picks: FramePick[],
    format: string,
    quality: number,
    outDir: string,
    baseName: string,
  ) => invoke<SaveResult>("save_frames", { picks, format, quality, outDir, baseName }),
  exportSegments: (
    source: string,
    segments: SegmentInput[],
    mode: string,
    outDir: string,
  ) => invoke<ExportResult>("export_segments", { source, segments, mode, outDir }),
  cancel: (job: string) => invoke<void>("cancel_job", { job }),
  reveal: (path: string) => invoke<void>("reveal_path", { path }),
};

export function mediaSrc(path: string): string {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}
