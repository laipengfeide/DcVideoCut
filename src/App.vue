<script setup lang="ts">
import { onMounted, ref } from "vue";
import AuthView from "./components/AuthView.vue";
import WorkspaceView from "./components/WorkspaceView.vue";
import ToastHost from "./components/ToastHost.vue";
import { api, type SessionView } from "./api";
import { errorText, toast } from "./toast";
import { ensureProgressListener } from "./progress";

const session = ref<SessionView | null>(null);
const loading = ref(true);

function setSession(value: SessionView) {
  session.value = value;
}

function signOut() {
  session.value = null;
}

onMounted(async () => {
  await ensureProgressListener();
  try {
    session.value = await api.bootstrap();
  } catch (err) {
    toast(`初始化失败：${errorText(err)}`, "error");
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <ToastHost />
  <div v-if="loading" class="boot-screen">
    <div class="spinner" />
    <span>正在启动 Dc Video Cut…</span>
  </div>
  <AuthView v-else-if="!session" @authenticated="setSession" />
  <WorkspaceView v-else :session="session" @session="setSession" @signout="signOut" />
</template>
