<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { api, mediaSrc, type FrameItem, type SessionView, type VideoMeta } from "../api";
import { baseNameOf, clamp, formatSize, formatTime } from "../format";
import { ensureProgressListener, progress, resetProgress } from "../progress";
import { errorText, toast } from "../toast";

const props = defineProps<{ video: VideoMeta; session: SessionView }>();

const intervalSec = ref(1);
const customInterval = ref("");
const extracting = ref(false);
const frames = ref<FrameItem[]>([]);
const selected = ref<number[]>([]);
const saveFormat = ref<"jpg" | "png">("jpg");
const quality = ref(2);
const outDir = ref(props.session.lastOutDir ?? "");
const saving = ref(false);
const savedFiles = ref<string[]>([]);
const savedDir = ref("");

const presets = [0.5, 1, 2, 5, 10];
const durationSec = computed(() => props.video.durationMs / 1000);

const effectiveInterval = computed(() => {
  const custom = Number(customInterval.value);
  if (customInterval.value.trim() && Number.isFinite(custom) && custom > 0) return custom;
  return intervalSec.value;
});

const estimateCount = computed(() =>
  Math.max(1, Math.ceil(durationSec.value / Math.max(effectiveInterval.value, 0.1))),
);

const frameLimitWarning = computed(
  () => props.session.frameLimitBytes !== null && props.video.sizeBytes > props.session.frameLimitBytes,
);

const selectedFrames = computed(() => frames.value.filter((f) => selected.value.includes(f.index)));

onMounted(() => {
  ensureProgressListener();
  intervalSec.value = defaultInterval();
});

function defaultInterval() {
  const target = clamp(durationSec.value / 36, 0.5, 30);
  return Number(target.toFixed(1)) || 1;
}

function choosePreset(value: number) {
  intervalSec.value = value;
  customInterval.value = "";
}

function toggle(frame: FrameItem) {
  const index = selected.value.indexOf(frame.index);
  if (index >= 0) selected.value.splice(index, 1);
  else selected.value.push(frame.index);
}

function selectAll() {
  selected.value = frames.value.map((f) => f.index);
}

function clearSelection() {
  selected.value = [];
}

async function extract() {
  if (extracting.value) return;
  if (frameLimitWarning.value) {
    toast("游客模式仅支持不超过 30MB 的视频抽帧，注册登录后可解除该限制", "error");
    return;
  }
  if (effectiveInterval.value < 0.1) {
    toast("抽帧间隔至少 0.1 秒", "error");
    return;
  }
  if (estimateCount.value > 3000) {
    toast("预计生成图片超过 3000 张，请加大抽帧间隔", "error");
    return;
  }
  extracting.value = true;
  frames.value = [];
  selected.value = [];
  savedFiles.value = [];
  resetProgress("正在抽取画面");
  try {
    const result = await api.extract(props.video.path, Math.round(effectiveInterval.value * 1000));
    frames.value = result.frames;
    selected.value = result.frames.map((f) => f.index);
    toast(`抽帧完成，共 ${result.frames.length} 张图片，已默认全选`, "success");
  } catch (err) {
    toast(errorText(err), "error");
  } finally {
    extracting.value = false;
  }
}

async function cancelExtract() {
  try {
    await api.cancel("");
  } catch (err) {
    toast(errorText(err), "error");
  }
}

async function pickOutDir() {
  try {
    const dir = await open({ directory: true, multiple: false, title: "选择图片保存目录" });
    if (typeof dir === "string") {
      outDir.value = dir;
      return dir;
    }
  } catch (err) {
    toast(errorText(err), "error");
  }
  return "";
}

async function save() {
  if (saving.value) return;
  if (selectedFrames.value.length === 0) {
    toast("请先勾选需要保存的图片", "error");
    return;
  }
  let dir = outDir.value;
  if (!dir) {
    dir = await pickOutDir();
    if (!dir) return;
  }
  saving.value = true;
  resetProgress("正在保存图片");
  try {
    const picks = selectedFrames.value.map((f) => ({
      file: f.file,
      index: f.index,
      timeMs: f.timeMs,
    }));
    const result = await api.saveFrames(
      picks,
      saveFormat.value,
      Math.round(quality.value),
      dir,
      baseNameOf(props.video.fileName),
    );
    savedFiles.value = result.saved;
    savedDir.value = result.outDir;
    outDir.value = result.outDir;
    toast(`已保存 ${result.saved.length} 张图片到 ${result.outDir}`, "success");
  } catch (err) {
    toast(errorText(err), "error");
  } finally {
    saving.value = false;
  }
}

async function revealDir() {
  try {
    await api.reveal(savedDir.value || outDir.value);
  } catch (err) {
    toast(errorText(err), "error");
  }
}
</script>

<template>
  <div class="panel-body">
    <div class="toolbar">
      <div class="toolbar-group">
        <span class="toolbar-label">抽帧间隔</span>
        <div class="interval-presets">
          <button
            v-for="p in presets"
            :key="p"
            class="preset-btn"
            :class="{ active: !customInterval && intervalSec === p }"
            @click="choosePreset(p)"
          >
            {{ p }}s
          </button>
        </div>
        <input
          v-model="customInterval"
          class="interval-input"
          type="number"
          min="0.1"
          step="0.1"
          placeholder="自定义"
          title="自定义间隔（秒）"
        />
      </div>
      <span class="estimate">
        预计 {{ estimateCount }} 张 · 视频 {{ formatTime(video.durationMs) }} · {{ formatSize(video.sizeBytes) }}
      </span>
      <div class="toolbar-spacer" />
      <span v-if="frameLimitWarning" class="chip amber">游客上限 30MB</span>
      <button v-if="extracting" class="btn btn-danger" @click="cancelExtract">取消抽帧</button>
      <button
        class="btn btn-primary"
        :disabled="extracting || frameLimitWarning || !session.ffmpegOk"
        @click="extract"
      >
        {{ extracting ? "抽帧中…" : frames.length ? "重新抽帧" : "开始抽帧" }}
      </button>
    </div>

    <div v-if="extracting || saving" class="progress-row">
      <div class="progress-track">
        <div class="progress-fill" :style="{ width: progress.percent + '%' }" />
      </div>
      <span class="progress-text">{{ Math.round(progress.percent) }}% · {{ progress.message }}</span>
    </div>

    <div class="frame-grid-wrap">
      <div v-if="extracting" class="empty-state">
        <div class="spinner" />
        <span>正在抽取画面，请稍候…</span>
      </div>
      <div v-else-if="frames.length === 0" class="empty-state">
        <span>还没有图片。设置间隔后点击「开始抽帧」。</span>
        <span style="font-size: 12px; color: #6b7c91">
          每一帧都会保存为一张独立图片，可自由勾选后批量导出
        </span>
      </div>
      <div v-else class="frame-grid">
        <button
          v-for="frame in frames"
          :key="frame.index"
          class="frame-card"
          :class="{ selected: selected.includes(frame.index) }"
          @click="toggle(frame)"
        >
          <img class="frame-thumb" :src="mediaSrc(frame.thumb)" :alt="formatTime(frame.timeMs)" loading="lazy" />
          <span class="frame-time">{{ formatTime(frame.timeMs) }}</span>
          <span class="frame-check">✓</span>
        </button>
      </div>
    </div>

    <div v-if="frames.length > 0" class="sel-bar">
      <span class="sel-count">
        已选 <b>{{ selected.length }}</b> / {{ frames.length }} 张
      </span>
      <button class="btn btn-sm btn-outline" @click="selectAll">全选</button>
      <button class="btn btn-sm btn-outline" @click="clearSelection">清空选择</button>

      <div class="save-options">
        <div class="switch-group">
          <button class="switch-item" :class="{ active: saveFormat === 'jpg' }" @click="saveFormat = 'jpg'">
            JPG
          </button>
          <button class="switch-item" :class="{ active: saveFormat === 'png' }" @click="saveFormat = 'png'">
            PNG
          </button>
        </div>
        <label v-if="saveFormat === 'jpg'" class="quality-box">
          质量
          <input v-model.number="quality" type="range" min="2" max="10" step="1" />
          <span>{{ quality === 2 ? "最高" : quality }}</span>
        </label>
        <div class="outdir">
          <span class="outdir-path" :title="outDir">{{ outDir || "未选择输出目录" }}</span>
          <button class="btn btn-sm btn-outline" @click="pickOutDir">目录</button>
        </div>
        <button class="btn btn-primary" :disabled="saving || selected.length === 0" @click="save">
          {{ saving ? "保存中…" : `保存 ${selected.length} 张` }}
        </button>
      </div>
    </div>

    <div v-if="savedFiles.length > 0" class="result-box">
      <span>已保存 {{ savedFiles.length }} 张图片到</span>
      <span style="color: #8ff3ce">{{ savedDir }}</span>
      <button class="btn btn-sm btn-outline" @click="revealDir">打开目录</button>
      <div class="result-files">
        <span v-for="f in savedFiles.slice(0, 8)" :key="f">{{ f }}</span>
        <span v-if="savedFiles.length > 8">… 等 {{ savedFiles.length }} 个文件</span>
      </div>
    </div>
  </div>
</template>
