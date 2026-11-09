<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { api, mediaSrc, type FrameItem, type SegmentInput, type SessionView, type VideoMeta } from "../api";
import { clamp, formatTime, parseTimeInput } from "../format";
import { ensureProgressListener, progress, resetProgress } from "../progress";
import { errorText, toast } from "../toast";
import Timeline from "./Timeline.vue";

const props = defineProps<{ video: VideoMeta; session: SessionView }>();

const duration = computed(() => props.video.durationMs / 1000);
const cuts = ref<number[]>([]);
const playhead = ref(0);
const thumbs = ref<FrameItem[]>([]);
const playing = ref(false);
const mode = ref<"copy" | "encode">("copy");
const manualStart = ref("");
const manualEnd = ref("");
const manualItems = ref<{ start: number; end: number }[]>([]);
const outDir = ref(props.session.lastOutDir ?? "");
const exporting = ref(false);
const outputs = ref<string[]>([]);
const videoEl = ref<HTMLVideoElement | null>(null);
const previewEnd = ref<number | null>(null);

const maxPoints = computed(() => props.session.maxCutPoints);
const pointsFull = computed(() => maxPoints.value !== null && cuts.value.length >= maxPoints.value);

const timelineThumbs = computed(() =>
  thumbs.value.map((t) => ({ src: mediaSrc(t.thumb), time: t.timeMs / 1000 })),
);

const segments = computed(() => {
  if (cuts.value.length === 0) return [] as { start: number; end: number }[];
  const points = [...cuts.value].sort((a, b) => a - b);
  const bounds = [0, ...points, duration.value];
  return bounds.slice(0, -1).map((start, i) => ({ start, end: bounds[i + 1] }));
});

const queue = computed(() => [
  ...segments.value.map((s) => ({ start: s.start, end: s.end, kind: "segment" as const })),
  ...manualItems.value.map((m) => ({ start: m.start, end: m.end, kind: "manual" as const })),
]);

onMounted(async () => {
  await ensureProgressListener();
  void loadThumbs();
});

async function loadThumbs() {
  resetProgress("正在生成时间轴缩略图");
  try {
    thumbs.value = await api.timelineThumbs(props.video.path, 18);
  } catch (err) {
    toast(`时间轴缩略图生成失败：${errorText(err)}`, "error");
  }
}

function seek(time: number) {
  playhead.value = clamp(time, 0, duration.value);
  if (videoEl.value) {
    try {
      videoEl.value.currentTime = playhead.value;
    } catch {
      /* ignore */
    }
  }
}

function togglePlay() {
  const el = videoEl.value;
  if (!el) return;
  if (el.paused) {
    previewEnd.value = null;
    void el.play();
    playing.value = true;
  } else {
    el.pause();
    playing.value = false;
  }
}

function onTimeUpdate() {
  const el = videoEl.value;
  if (!el) return;
  if (!el.paused) playhead.value = el.currentTime;
  if (previewEnd.value !== null && el.currentTime >= previewEnd.value) {
    el.pause();
    playing.value = false;
    playhead.value = el.currentTime;
    previewEnd.value = null;
  }
}

function preview(start: number, end: number) {
  const el = videoEl.value;
  if (!el) return;
  try {
    el.currentTime = start;
  } catch {
    /* ignore */
  }
  previewEnd.value = end;
  playhead.value = start;
  void el.play();
  playing.value = true;
}

function insertAt(time: number) {
  if (pointsFull.value) {
    toast(
      `游客模式最多插入 ${maxPoints.value} 个切割点（切成 3 段），注册登录后可无限切割`,
      "error",
    );
    return;
  }
  if (time <= 0.1 || time >= duration.value - 0.1) {
    toast("请在视频中间位置插入切割点", "error");
    return;
  }
  if (cuts.value.some((c) => Math.abs(c - time) < 0.2)) {
    toast("该位置附近已经有切割点了", "info");
    return;
  }
  cuts.value.push(time);
  cuts.value.sort((a, b) => a - b);
  toast(
    `已在 ${formatTime(time * 1000)} 处插入切割点：${cuts.value.length} 个点 / ${cuts.value.length + 1} 段`,
    "success",
  );
}

function insertAtPlayhead() {
  insertAt(playhead.value);
}

function moveCut(payload: { index: number; time: number }) {
  const index = payload.index;
  if (index < 0 || index >= cuts.value.length) return;
  const lower = index > 0 ? cuts.value[index - 1] + 0.2 : 0.1;
  const upper =
    index < cuts.value.length - 1 ? cuts.value[index + 1] - 0.2 : duration.value - 0.1;
  cuts.value[index] = clamp(payload.time, lower, Math.max(lower, upper));
}

function removeCut(index: number) {
  if (index < 0 || index >= cuts.value.length) return;
  cuts.value.splice(index, 1);
  toast("已删除切割点", "info");
}

function removeLastCut() {
  if (cuts.value.length === 0) return;
  cuts.value.pop();
}

function addManual() {
  const startMs = parseTimeInput(manualStart.value);
  const endMs = parseTimeInput(manualEnd.value);
  if (startMs === null || endMs === null) {
    toast("请输入有效的开始 / 结束时间，例如 00:12.5 或 12.5（单位秒）", "error");
    return;
  }
  if (startMs >= endMs) {
    toast("结束时间必须大于开始时间", "error");
    return;
  }
  if (endMs > props.video.durationMs + 100) {
    toast("结束时间超出视频时长", "error");
    return;
  }
  manualItems.value.push({
    start: startMs / 1000,
    end: Math.min(endMs, props.video.durationMs) / 1000,
  });
  manualStart.value = "";
  manualEnd.value = "";
  toast("已添加到导出列表", "success");
}

function fillFromPlayhead(which: "start" | "end") {
  const value = formatTime(playhead.value * 1000, true);
  if (which === "start") manualStart.value = value;
  else manualEnd.value = value;
}

function removeManual(index: number) {
  manualItems.value.splice(index, 1);
}

async function pickOutDir() {
  try {
    const dir = await open({ directory: true, multiple: false, title: "选择片段输出目录" });
    if (typeof dir === "string") {
      outDir.value = dir;
      return dir;
    }
  } catch (err) {
    toast(errorText(err), "error");
  }
  return "";
}

async function doExport(items: { start: number; end: number }[]) {
  if (exporting.value) return;
  if (items.length === 0) {
    toast("请先插入切割点或添加手动时间区间", "error");
    return;
  }
  const limit = props.session.maxCutPoints !== null ? props.session.maxCutPoints + 1 : 40;
  if (items.length > limit) {
    toast(
      props.session.isGuest
        ? "游客模式最多只能切成 3 段（插入 2 个点），请注册登录"
        : "单次最多导出 40 个片段",
      "error",
    );
    return;
  }
  let dir = outDir.value;
  if (!dir) {
    dir = await pickOutDir();
    if (!dir) return;
  }
  exporting.value = true;
  outputs.value = [];
  resetProgress("正在导出片段");
  try {
    const inputs: SegmentInput[] = items.map((item) => ({
      startMs: Math.round(item.start * 1000),
      endMs: Math.round(item.end * 1000),
    }));
    const result = await api.exportSegments(props.video.path, inputs, mode.value, dir);
    outputs.value = result.outputs;
    outDir.value = result.outDir;
    toast(`导出完成，共 ${result.outputs.length} 个片段`, "success");
  } catch (err) {
    toast(errorText(err), "error");
  } finally {
    exporting.value = false;
  }
}

async function cancelExport() {
  try {
    await api.cancel("");
  } catch (err) {
    toast(errorText(err), "error");
  }
}

async function revealOutput() {
  try {
    await api.reveal(outputs.value[0] ?? outDir.value);
  } catch (err) {
    toast(errorText(err), "error");
  }
}
</script>

<template>
  <div class="panel-body">
    <div class="cut-layout">
      <div class="player-col">
        <div class="player">
          <video
            ref="videoEl"
            :src="mediaSrc(video.path)"
            preload="auto"
            @timeupdate="onTimeUpdate"
            @play="playing = true"
            @pause="playing = false"
            @click="togglePlay"
          />
          <div class="player-controls">
            <button class="play-btn" :title="playing ? '暂停' : '播放'" @click.stop="togglePlay">
              <svg v-if="!playing" width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                <path d="M7 4l14 8-14 8z" />
              </svg>
              <svg v-else width="11" height="11" viewBox="0 0 24 24" fill="currentColor">
                <path d="M6 4h5v16H6zM13 4h5v16h-5z" />
              </svg>
            </button>
            <span class="player-time">
              {{ formatTime(playhead * 1000, true) }} / {{ formatTime(video.durationMs) }}
            </span>
            <span class="ctrl-note">点击视频可播放 / 暂停</span>
          </div>
        </div>

        <Timeline
          :duration="duration"
          :playhead="playhead"
          :cuts="cuts"
          :thumbs="timelineThumbs"
          @seek="seek"
          @move-cut="moveCut"
          @remove-cut="removeCut"
          @insert="insertAt"
        />

        <div class="cut-controls">
          <button class="btn btn-primary" :disabled="pointsFull" @click="insertAtPlayhead">
            在播放头插入切割点
          </button>
          <button class="btn btn-sm btn-outline" @click="seek(playhead - 1)">后退 1s</button>
          <button class="btn btn-sm btn-outline" @click="seek(playhead + 1)">前进 1s</button>
          <button
            v-if="cuts.length > 0"
            class="btn btn-sm btn-outline"
            @click="removeLastCut"
          >
            删除最后一点
          </button>
          <span class="toolbar-spacer" />
          <span class="ctrl-note">双击时间轴任意位置可直接插入切割点 · 拖动蓝色标记微调</span>
        </div>
      </div>

      <div class="side-col">
        <div class="side-card">
          <div class="side-title">
            <span>切割点</span>
            <span class="st-hint">
              {{ cuts.length }} 个点 → {{ cuts.length + 1 }} 段
              <template v-if="maxPoints !== null"> · 游客上限 {{ maxPoints }}</template>
            </span>
          </div>
          <div v-if="cuts.length === 0" class="export-note">
            还没有切割点。拖动时间轴到目标位置后点击「在播放头插入切割点」，插入 1 个点切成 2 段，2 个点切成 3 段。
          </div>
          <div v-else class="points-list">
            <span v-for="(cut, index) in cuts" :key="index" class="point-item">
              {{ formatTime(cut * 1000) }}
              <button title="删除该切割点" @click="removeCut(index)">✕</button>
            </span>
          </div>
        </div>

        <div class="side-card">
          <div class="side-title">
            <span>手动时间裁剪</span>
            <span class="st-hint">手动输入开始 / 结束时间</span>
          </div>
          <div class="manual-row">
            <input v-model="manualStart" class="time-input" placeholder="开始 00:05.5" />
            <button class="link-btn" title="取当前播放头" @click="fillFromPlayhead('start')">取点</button>
          </div>
          <div class="manual-row">
            <input v-model="manualEnd" class="time-input" placeholder="结束 00:12.0" />
            <button class="link-btn" title="取当前播放头" @click="fillFromPlayhead('end')">取点</button>
          </div>
          <div class="manual-actions">
            <button class="btn btn-sm" @click="addManual">添加到导出列表</button>
            <button class="btn btn-sm btn-outline" @click="manualStart = ''; manualEnd = ''">清空</button>
          </div>
          <div v-if="manualItems.length > 0" class="segment-list">
            <div v-for="(item, index) in manualItems" :key="index" class="segment-row">
              <span class="seg-index">手</span>
              <span class="seg-times">
                {{ formatTime(item.start * 1000) }} → {{ formatTime(item.end * 1000) }}
              </span>
              <span class="seg-dur">{{ (item.end - item.start).toFixed(2) }}s</span>
              <span class="seg-actions">
                <button class="link-btn" @click="preview(item.start, item.end)">预览</button>
                <button class="link-btn danger" @click="removeManual(index)">删除</button>
              </span>
            </div>
          </div>
        </div>

        <div class="side-card">
          <div class="side-title">
            <span>导出片段</span>
            <span class="st-hint">{{ queue.length }} 个待导出</span>
          </div>

          <div class="mode-row">
            <div class="radio-card" :class="{ active: mode === 'copy' }" @click="mode = 'copy'">
              <span class="radio-mark" />
              <div>
                <div class="rc-title">快速切割</div>
                <div class="rc-desc">不重新编码，速度快、无画质损失，起点会对齐到关键帧</div>
              </div>
            </div>
            <div class="radio-card" :class="{ active: mode === 'encode' }" @click="mode = 'encode'">
              <span class="radio-mark" />
              <div>
                <div class="rc-title">精确切割</div>
                <div class="rc-desc">重新编码 H.264，帧级精准，速度较慢</div>
              </div>
            </div>
          </div>

          <div class="manual-row">
            <span class="outdir-path" :title="outDir" style="flex: 1; max-width: none">
              {{ outDir || "未选择输出目录" }}
            </span>
            <button class="btn btn-sm btn-outline" @click="pickOutDir">选择目录</button>
          </div>

          <div v-if="segments.length > 0" class="segment-list">
            <div v-for="(seg, index) in segments" :key="'seg' + index" class="segment-row">
              <span class="seg-index">{{ index + 1 }}</span>
              <span class="seg-times">
                {{ formatTime(seg.start * 1000) }} → {{ formatTime(seg.end * 1000) }}
              </span>
              <span class="seg-dur">{{ (seg.end - seg.start).toFixed(2) }}s</span>
              <span class="seg-actions">
                <button class="link-btn" @click="preview(seg.start, seg.end)">预览</button>
                <button class="link-btn" @click="doExport([seg])">导出</button>
              </span>
            </div>
          </div>

          <div class="export-bar">
            <button
              class="btn btn-primary"
              :disabled="exporting || queue.length === 0"
              @click="doExport(queue)"
            >
              {{ exporting ? "导出中…" : `导出全部（${queue.length} 段）` }}
            </button>
            <button v-if="exporting" class="btn btn-danger" @click="cancelExport">取消</button>
          </div>

          <div v-if="exporting" class="progress-row" style="padding: 0">
            <div class="progress-track">
              <div class="progress-fill" :style="{ width: progress.percent + '%' }" />
            </div>
            <span class="progress-text">{{ Math.round(progress.percent) }}% · {{ progress.message }}</span>
          </div>

          <div v-if="outputs.length > 0" class="result-box">
            <span>已导出 {{ outputs.length }} 个片段</span>
            <button class="btn btn-sm btn-outline" @click="revealOutput">打开所在文件夹</button>
            <div class="result-files">
              <span v-for="file in outputs" :key="file">{{ file }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
