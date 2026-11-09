<script setup lang="ts">
import { computed, ref } from "vue";
import { formatTime } from "../format";

const props = defineProps<{
  duration: number;
  playhead: number;
  cuts: number[];
  thumbs: { src: string; time: number }[];
}>();

const emit = defineEmits<{
  (e: "seek", time: number): void;
  (e: "move-cut", payload: { index: number; time: number }): void;
  (e: "remove-cut", index: number): void;
  (e: "insert", time: number): void;
}>();

const root = ref<HTMLElement | null>(null);
const dragMode = ref<"playhead" | number | null>(null);

function pct(time: number): number {
  if (props.duration <= 0) return 0;
  return (time / props.duration) * 100;
}

const ticks = computed(() => {
  const total = props.duration;
  if (total <= 0) return [] as number[];
  const raw = total / 6;
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600];
  const step = steps.find((s) => s >= raw) ?? 3600;
  const list: number[] = [];
  for (let t = 0; t <= total + 0.001; t += step) list.push(t);
  return list;
});

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function locate(clientX: number): number {
  const el = root.value;
  if (!el) return 0;
  const rect = el.getBoundingClientRect();
  if (rect.width === 0) return 0;
  return clamp01((clientX - rect.left) / rect.width) * props.duration;
}

function onDown(event: PointerEvent) {
  const el = root.value;
  if (!el) return;
  el.setPointerCapture(event.pointerId);
  dragMode.value = "playhead";
  emit("seek", locate(event.clientX));
}

function startCutDrag(event: PointerEvent, index: number) {
  event.stopPropagation();
  const el = root.value;
  if (!el) return;
  el.setPointerCapture(event.pointerId);
  dragMode.value = index;
}

function onMove(event: PointerEvent) {
  if (dragMode.value === null) return;
  const time = locate(event.clientX);
  if (dragMode.value === "playhead") emit("seek", time);
  else emit("move-cut", { index: dragMode.value, time });
}

function onUp(event: PointerEvent) {
  const el = root.value;
  if (el && el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId);
  dragMode.value = null;
}

function onDoubleClick(event: MouseEvent) {
  emit("insert", locate(event.clientX));
}

function shortTick(time: number): string {
  return formatTime(time * 1000).replace(/^0+:/, "");
}
</script>

<template>
  <div class="timeline-wrap">
    <div
      ref="root"
      class="tl"
      @pointerdown="onDown"
      @pointermove="onMove"
      @pointerup="onUp"
      @pointercancel="onUp"
      @dblclick="onDoubleClick"
    >
      <div class="tl-strip">
        <div
          v-for="(thumb, i) in thumbs"
          :key="i"
          class="tl-cell"
          :style="{ backgroundImage: `url('${thumb.src}')` }"
        />
        <div v-if="thumbs.length === 0" class="tl-empty">时间轴缩略图生成中…</div>
      </div>
      <div class="tl-shade" />
      <div class="tl-playhead" :style="{ left: pct(playhead) + '%' }" />
      <div
        v-for="(cut, index) in cuts"
        :key="'cut-' + index"
        class="tl-cut"
        :style="{ left: pct(cut) + '%' }"
        @pointerdown="startCutDrag($event, index)"
        @dblclick.stop
        @contextmenu.prevent="emit('remove-cut', index)"
        :title="'拖动调整 / 右键删除 ' + formatTime(cut * 1000)"
      >
        <i class="tl-cut-line" />
        <span class="tl-cut-dot">{{ formatTime(cut * 1000) }}</span>
      </div>
    </div>
    <div class="tl-scale">
      <span
        v-for="tick in ticks"
        :key="tick"
        class="tl-tick"
        :style="{ left: pct(tick) + '%' }"
      >
        {{ shortTick(tick) }}
      </span>
    </div>
  </div>
</template>
