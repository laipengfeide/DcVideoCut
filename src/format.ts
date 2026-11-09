export function formatTime(ms: number, withMillis = false): string {
  const safe = Math.max(0, Math.round(ms));
  const total = Math.floor(safe / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const millis = safe % 1000;
  const base = `${h > 0 ? String(h).padStart(2, "0") + ":" : ""}${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return withMillis ? `${base}.${String(millis).padStart(3, "0")}` : base;
}

export function formatTimeShort(ms: number): string {
  return formatTime(ms).replace(/^00:/, "");
}

/** 解析 "12.5" / "00:12.5" / "01:02:03" / "1h2m3s" 等输入为毫秒，失败返回 null */
export function parseTimeInput(input: string): number | null {
  const text = input.trim().toLowerCase();
  if (!text) return null;

  const hms = text.match(/^(\d{1,3}):(\d{1,2})(?::(\d{1,2}(?:\.\d{1,3})?))?$/);
  if (hms) {
    const a = Number(hms[1]);
    const b = Number(hms[2]);
    const c = hms[3] ? Number(hms[3]) : null;
    if (c === null) {
      return Math.round(a * 60 * 1000 + b * 1000);
    }
    return Math.round(a * 3600 * 1000 + b * 60 * 1000 + c * 1000);
  }

  const unit = text.match(/^(?:(\d{1,3})h)?(?:(\d{1,3})m)?(?:(\d{1,3}(?:\.\d{1,3})?)s)?$/);
  if (unit && (unit[1] || unit[2] || unit[3])) {
    const h = Number(unit[1] || 0);
    const m = Number(unit[2] || 0);
    const s = Number(unit[3] || 0);
    return Math.round(h * 3600000 + m * 60000 + s * 1000);
  }

  const seconds = Number(text);
  if (!Number.isNaN(seconds) && seconds >= 0) {
    return Math.round(seconds * 1000);
  }
  return null;
}

export function formatSize(bytes: number): string {
  const kb = 1024;
  const mb = kb * 1024;
  const gb = mb * 1024;
  if (bytes >= gb) return `${(bytes / gb).toFixed(2)} GB`;
  if (bytes >= mb) return `${(bytes / mb).toFixed(1)} MB`;
  if (bytes >= kb) return `${Math.round(bytes / kb)} KB`;
  return `${bytes} B`;
}

export function baseNameOf(fileName: string): string {
  const dot = fileName.lastIndexOf(".");
  const name = dot > 0 ? fileName.slice(0, dot) : fileName;
  return name.replace(/[\\/:*?"<>|]/g, "_");
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
