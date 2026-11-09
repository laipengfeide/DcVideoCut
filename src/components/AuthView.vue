<script setup lang="ts">
import { ref } from "vue";
import { api, type SessionView } from "../api";
import { errorText, toast } from "../toast";
import { formatSize } from "../format";

const emit = defineEmits<{ (e: "authenticated", session: SessionView): void }>();

const tab = ref<"login" | "register">("login");
const username = ref("");
const password = ref("");
const busy = ref(false);

async function submit() {
  if (busy.value) return;
  if (!username.value.trim()) {
    toast("请输入用户名", "error");
    return;
  }
  if (!password.value) {
    toast("请输入密码", "error");
    return;
  }
  busy.value = true;
  try {
    const session =
      tab.value === "login"
        ? await api.login(username.value, password.value)
        : await api.register(username.value, password.value);
    toast(tab.value === "login" ? "登录成功，欢迎回来" : "注册成功，限制已解除", "success");
    emit("authenticated", session);
  } catch (err) {
    toast(errorText(err), "error");
  } finally {
    busy.value = false;
  }
}

async function enterGuest() {
  if (busy.value) return;
  busy.value = true;
  try {
    const session = await api.guest();
    emit("authenticated", session);
  } catch (err) {
    toast(errorText(err), "error");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="auth-page">
    <div class="auth-card">
      <aside class="auth-hero">
        <div class="brand-mark">
          <svg viewBox="0 0 24 24" fill="none">
            <path d="M5 4.5v15l11-7.5-11-7.5z" fill="#35d39a" />
            <path d="M19 4v16" stroke="#5aa7f7" stroke-width="1.6" stroke-dasharray="3 2.4" />
          </svg>
        </div>
        <h1 class="brand-title">Dc Video Cut</h1>
        <p class="brand-sub">
          一个轻量的桌面视频工作台：把视频拆成一帧帧图片按需保存，或在时间轴上插点把视频切成多段导出。
        </p>

        <div class="feature-list">
          <div class="feature-item">
            <span class="feature-icon">1</span>
            <span>按秒抽帧，一帧一张图片，自由勾选后批量保存为 JPG / PNG。</span>
          </div>
          <div class="feature-item">
            <span class="feature-icon">2</span>
            <span>时间轴插点即分割，插入 1 个点切成 2 段，2 个点切成 3 段，以此类推。</span>
          </div>
          <div class="feature-item">
            <span class="feature-icon">3</span>
            <span>同时支持手动输入开始 / 结末时间直接裁剪导出。</span>
          </div>
          <div class="feature-item">
            <span class="feature-icon">4</span>
            <span>本地处理、离线可用，视频与图片不会上传到任何服务器。</span>
          </div>
        </div>

        <div class="auth-limits">
          <div class="lm-row">
            <span>游客：切割点</span><b>最多 2 个（切成 3 段）</b>
          </div>
          <div class="lm-row">
            <span>游客：抽帧视频</span><b>不超过 30 MB</b>
          </div>
          <div class="lm-row">
            <span>注册用户</span><b>不限切割点、不限抽帧视频大小</b>
          </div>
          <div class="lm-row">
            <span>所有用户共同上限</span><b>单个视频 {{ formatSize(2 * 1024 * 1024 * 1024) }}</b>
          </div>
        </div>
      </aside>

      <section class="auth-form">
        <div class="auth-tabs">
          <button class="auth-tab" :class="{ active: tab === 'login' }" @click="tab = 'login'">
            登录
          </button>
          <button
            class="auth-tab"
            :class="{ active: tab === 'register' }"
            @click="tab = 'register'"
          >
            注册
          </button>
        </div>

        <form class="auth-form" style="padding: 0; gap: 14px" @submit.prevent="submit">
          <div class="field">
            <span class="label">用户名</span>
            <input
              v-model="username"
              class="input"
              type="text"
              autocomplete="username"
              placeholder="3-24 个字符，不含空格"
              maxlength="24"
            />
          </div>
          <div class="field">
            <span class="label">密码</span>
            <input
              v-model="password"
              class="input"
              type="password"
              autocomplete="current-password"
              placeholder="至少 6 位"
              maxlength="128"
            />
          </div>
          <button class="btn btn-primary" type="submit" :disabled="busy" style="height: 44px">
            {{ busy ? "处理中…" : tab === "login" ? "登录并开始" : "注册并开始" }}
          </button>
        </form>

        <p class="auth-hint">
          注册后可解除「30MB 抽帧上限」与「2 个切割点」限制，账号数据仅保存在本机 SQLite 数据库中。
        </p>

        <div class="divider">或</div>

        <button class="guest-btn" :disabled="busy" @click="enterGuest">
          以游客身份进入（可体验抽帧与切割）
        </button>
        <p class="guest-note">
          游客模式下抽帧视频需 ≤ 30MB、最多插入 2 个切割点；任何模式下单个视频都不能超过 2GB。
        </p>
      </section>
    </div>
  </div>
</template>
