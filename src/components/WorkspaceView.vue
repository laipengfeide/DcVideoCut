<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type SessionView, type VideoMeta } from "../api";
import { errorText, toast } from "../toast";
import { formatSize, formatTime } from "../format";
import ExtractPanel from "./ExtractPanel.vue";
import CutPanel from "./CutPanel.vue";

const props = defineProps<{ session: SessionView }>();
const emit = defineEmits<{
  (e: "session", value: SessionView): void;
  (e: "signout"): void;
}>();

const VIDEO_EXT = [
  "mp4", "mov", "mkv", "avi", "webm", "flv", "wmv", "m4v", "ts", "mpg", "mpeg", "3gp", "m2ts", "ogv",
];

const video = ref<VideoMeta | null>(null);
const loading = ref(false);
const dragging = ref(false);
const tab = ref<"extract" | "cut">("extract");

function isVideo(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return VIDEO_EXT.includes(ext);
}

async function pickFile() {
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "选择视频文件",
      filters: [{ name: "视频文件", extensions: VIDEO_EXT }],
    });
    if (typeof selected === "string") await loadVideo(selected);
  } catch (err) {
    toast(errorText(err), "error");
  }
}

async function loadVideo(path: string) {
  if (loading.value) return;
  if (!isVideo(path)) {
    toast("请选择常见视频文件（mp4 / mov / mkv / avi / webm 等）", "error");
    return;
  }
  if (!props.session.ffmpegOk) {
    toast(props.session.ffmpegMessage, "error");
    return;
  }
  loading.value = true;
  try {
    const meta = await api.probe(path);
    video.value = meta;
    tab.value = "extract";
    if (props.session.frameLimitBytes && meta.sizeBytes > props.session.frameLimitBytes) {
      toast(
        `该视频为 ${formatSize(meta.sizeBytes)}，超过游客抽帧上限 30MB，可正常切割；抽帧请注册登录`,
        "info",
      );
    } else {
      toast("视频已载入", "success");
    }
  } catch (err) {
    toast(errorText(err), "error");
  } finally {
    loading.value = false;
  }
}

let unlisten: (() => void) | null = null;

onMounted(async () => {
  try {
    unlisten = await getCurrentWebview().onDragDropEvent(async (event) => {
      const payload = event.payload as unknown as {
        type: string;
        paths?: string[];
      };
      if (payload.type === "enter" || payload.type === "over") {
        dragging.value = true;
      } else if (payload.type === "leave") {
        dragging.value = false;
      } else if (payload.type === "drop") {
        dragging.value = false;
        const first = payload.paths?.[0];
        if (first) await loadVideo(first);
      }
    });
  } catch {
    /* 拖拽不可用时仍可通过按钮选择文件 */
  }
});

onUnmounted(() => {
  unlisten?.();
});

async function switchAccount() {
  try {
    const session = await api.logout();
    emit("session", session);
    emit("signout");
  } catch (err) {
    toast(errorText(err), "error");
  }
}

function closeVideo() {
  video.value = null;
}

async function revealFile() {
  if (!video.value) return;
  try {
    await api.reveal(video.value.path);
  } catch (err) {
    toast(errorText(err), "error");
  }
}
</script>

<template>
  <div class="workspace">
    <header class="topbar">
      <div class="brand">
        <div class="brand-mark" style="width: 36px; height: 36px; border-radius: 10px">
          <svg viewBox="0 0 24 24" fill="none">
            <path d="M5 4.5v15l11-7.5-11-7.5z" fill="#35d39a" />
            <path d="M19 4v16" stroke="#5aa7f7" stroke-width="1.6" stroke-dasharray="3 2.4" />
          </svg>
        </div>
        <div class="brand-text">
          <div class="t">Dc Video Cut</div>
          <div class="s">本地视频抽帧 · 时间轴切割</div>
        </div>
      </div>

      <div class="topbar-right">
        <span class="chip" :class="session.isGuest ? 'amber' : 'mint'">
          <span class="dot" />
          {{ session.isGuest ? "游客模式" : session.username }}
        </span>
        <span class="chip blue">
          抽帧 {{ session.frameLimitBytes ? "≤ " + formatSize(session.frameLimitBytes) : "不限大小" }}
        </span>
        <span class="chip blue">
          切割点 {{ session.maxCutPoints ?? "不限" }}
        </span>
        <span class="chip">单视频 ≤ {{ formatSize(session.maxVideoBytes) }}</span>
        <button class="btn btn-sm btn-outline" @click="switchAccount">切换账号</button>
      </div>
    </header>

    <div v-if="!session.ffmpegOk" class="banner error">
      <strong>未就绪：</strong><span style="user-select: text">{{ session.ffmpegMessage }}</span>
    </div>

    <div v-if="!video" class="dropzone" :class="{ dragging }">
      <div class="dropzone-inner">
        <div class="dz-icon">
          <svg width="38" height="38" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6">
            <rect x="3" y="4" width="18" height="16" rx="3" />
            <path d="M7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4" stroke-dasharray="2 2" />
          </svg>
        </div>
        <div class="dz-title">{{ loading ? "正在读取视频信息…" : "把视频拖到这里，或点击选择" }}</div>
        <div class="dz-hint">
          抽帧：按固定间隔把画面导出成一帧一张的图片，可勾选后保存为 JPG / PNG。<br />
          切割：拖动时间轴插入切割点，或手动输入开始 / 结束时间导出片段。
        </div>
        <div class="dz-actions">
          <button class="btn btn-primary" :disabled="loading || !session.ffmpegOk" @click="pickFile">
            {{ loading ? "读取中…" : "选择视频文件" }}
          </button>
        </div>
        <div class="dz-format">
          <span v-for="f in ['mp4', 'mov', 'mkv', 'avi', 'webm', 'flv', 'wmv', 'ts']" :key="f" class="chip">{{ f }}</span>
        </div>
      </div>
    </div>

    <div v-else class="studio">
      <div class="video-bar">
        <div class="video-info">
          <div class="video-name" :title="video.path">{{ video.fileName }}</div>
          <div class="video-meta">
            <span class="chip mint">{{ formatTime(video.durationMs) }}</span>
            <span class="chip">{{ video.width }} × {{ video.height }}</span>
            <span class="chip">{{ video.fps ? video.fps.toFixed(2) + " fps" : "未知帧率" }}</span>
            <span class="chip">{{ video.videoCodec }}{{ video.audioCodec ? " / " + video.audioCodec : " / 无音轨" }}</span>
            <span class="chip">{{ formatSize(video.sizeBytes) }}</span>
            <span
              v-if="session.frameLimitBytes && video.sizeBytes > session.frameLimitBytes"
              class="chip amber"
            >
              超过游客抽帧上限
            </span>
          </div>
        </div>
        <button class="btn btn-sm btn-outline" @click="revealFile">在文件夹中显示</button>
        <button class="btn btn-sm btn-outline" @click="closeVideo">关闭视频</button>
        <button class="btn btn-sm btn-primary" :disabled="loading" @click="pickFile">更换视频</button>
      </div>

      <div class="tabs">
        <button class="tab" :class="{ active: tab === 'extract' }" @click="tab = 'extract'">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7">
            <rect x="3" y="3" width="18" height="18" rx="3" />
            <circle cx="9" cy="9" r="2" />
            <path d="M21 15l-5-5-9 9" />
          </svg>
          抽帧图片
        </button>
        <button class="tab" :class="{ active: tab === 'cut' }" @click="tab = 'cut'">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7">
            <circle cx="6" cy="6" r="2.6" />
            <circle cx="6" cy="18" r="2.6" />
            <path d="M8 7.6L20 18M8 16.4L20 6" />
          </svg>
          视频切割
        </button>
      </div>

      <ExtractPanel v-if="tab === 'extract'" :key="'extract-' + video.path" :video="video" :session="session" />
      <CutPanel v-else :key="'cut-' + video.path" :video="video" :session="session" />
    </div>
  </div>
</template>
