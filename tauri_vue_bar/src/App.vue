<template>
  <!-- 加载态 -->
  <div v-if="!monitorSnapshot" class="button-row">Loading...</div>

  <!-- 主界面 -->
  <div v-else class="button-row">
    <div class="buttons-container">
      <!-- Tag Buttons -->
      <button
        v-for="(emoji, i) in BUTTONS"
        :key="i"
        :class="buttonClass(i)"
        @mousedown="pressedButton = i"
        @mouseup="onTagRelease(i)"
        @mouseleave="pressedButton = null"
      >
        {{ emoji }}
      </button>

      <!-- 布局切换 -->
      <div class="layout-controls">
        <div
          :class="['pill', 'layout-toggle', layoutOpen ? 'open' : 'closed']"
          @click="layoutOpen = !layoutOpen"
          title="切换布局"
        >
          {{ currentSymbol }}
        </div>
        <div v-if="layoutOpen" class="layout-selector">
          <div
            :class="['pill', 'layout-option', currentSymbol === '[]=' ? 'current' : '']"
            @click="onLayoutSelect(0)"
          >
            []=
          </div>
          <div
            :class="['pill', 'layout-option', currentSymbol === '><>' ? 'current' : '']"
            @click="onLayoutSelect(1)"
          >
            &gt;&lt;&gt;
          </div>
          <div
            :class="['pill', 'layout-option', currentSymbol === '[M]' ? 'current' : '']"
            @click="onLayoutSelect(2)"
          >
            [M]
          </div>
        </div>
      </div>
    </div>

    <div class="spacer"></div>

    <div class="right-info-container">
      <!-- 系统信息 -->
      <template v-if="systemSnapshot">
        <div class="system-info-container">
          <div class="pill usage-pill" :class="cpuClass" title="CPU 平均使用率">
            CPU {{ Math.round(systemSnapshot.cpu_average) }}%
          </div>
          <div
            class="pill usage-pill"
            :class="memClass"
            :title="`内存使用: ${formatBytes(systemSnapshot.memory_used)} / ${formatBytes(systemSnapshot.memory_total)}`"
          >
            MEM {{ Math.round(systemSnapshot.memory_usage_percent) }}%
          </div>
          <div
            class="pill usage-pill"
            :class="battClass"
            :title="systemSnapshot.is_charging
              ? `电池充电中: ${systemSnapshot.battery_percent.toFixed(1)}%`
              : `电池电量: ${systemSnapshot.battery_percent.toFixed(1)}%`"
          >
            {{ systemSnapshot.is_charging ? '🔌' : '🔋' }}
            {{ Math.round(systemSnapshot.battery_percent) }}%
          </div>
        </div>
      </template>
      <template v-else>
        <div class="system-info-container">
          <div class="pill usage-pill usage-warn">CPU --%</div>
          <div class="pill usage-pill usage-warn">MEM --%</div>
          <div class="pill usage-pill usage-warn">🔋 --%</div>
        </div>
      </template>

      <!-- 截图按钮 -->
      <div
        class="pill screenshot-pill"
        :class="{ taking: isTaking }"
        @click="onScreenshot"
        title="截图 (Flameshot)"
      >
        {{ isTaking ? '⏳' : '📸' }}
      </div>

      <!-- 时间 -->
      <div
        class="pill time-pill"
        @click="showSeconds = !showSeconds"
        title="点击切换秒显示"
      >
        {{ formattedTime }}
      </div>

      <!-- 显示器/缩放 -->
      <div class="pill monitor-pill" title="显示器">
        🖥️ {{ monitorIcon(monitorNum) }}
      </div>
      <div class="pill scale-pill" title="Scale Factor">
        s: {{ scaleText }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

// --- 类型定义，与后端 Rust 结构体对应 ---
interface TagStatus {
  is_selected: boolean;
  is_urg: boolean;
  is_filled: boolean;
  is_occ: boolean;
}

interface MonitorInfoSnapshot {
  monitor_num: number;
  monitor_width: number;
  monitor_height: number;
  monitor_x: number;
  monitor_y: number;
  tag_status_vec: TagStatus[];
  client_name: string;
  ltsymbol: string; // 形如: "[]=" 或 "[]=" + " s: 1.00, m: 0"
}

interface SystemSnapshot {
  cpu_average: number;
  memory_used: number;
  memory_total: number;
  memory_usage_percent: number;
  battery_percent: number;
  is_charging: boolean;
}

// --- 帮助函数 & 常量 ---
const BUTTONS = ['🐖', '🐄', '🐂', '🐃', '🦥', '🦣', '🐏', '🦆', '🐢'];

const getButtonClass = (tagStatus: TagStatus): string => {
  if (tagStatus.is_filled) return 'emoji-button state-filtered';
  if (tagStatus.is_selected) return 'emoji-button state-selected';
  if (tagStatus.is_urg) return 'emoji-button state-urgent';
  if (tagStatus.is_occ) return 'emoji-button state-occupied';
  return 'emoji-button state-default';
};

const formatBytes = (bytes: number): string => {
  if (bytes === 0) return '0B';
  const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const size = parseFloat((bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1));
  return `${size}${UNITS[i]}`;
};

function parseLtSymbol(lts?: string) {
  if (!lts) return { symbol: '[]=', scale: undefined as number | undefined };
  const symbolMatch = lts.match(/^(\S+)/);
  const scaleMatch = lts.match(/s:\s*([0-9.]+)/i);
  const symbol = symbolMatch ? symbolMatch[1] : '[]=';
  const scale = scaleMatch ? parseFloat(scaleMatch[1]) : undefined;
  return { symbol, scale };
}

function monitorIcon(num: number) {
  // Nerd Font 字体存在时会显示图标，否则你可以改为 `M${num}`
  if (num === 0) return '󰎡';
  if (num === 1) return '󰎤';
  return `M${num}`;
}

// --- 响应式状态 ---
const monitorSnapshot = ref<MonitorInfoSnapshot | null>(null);
const systemSnapshot = ref<SystemSnapshot | null>(null);

// Tag 按钮按压态
const pressedButton = ref<number | null>(null);

// 布局下拉开合
const layoutOpen = ref(false);

// 截图执行态
const isTaking = ref(false);

// 时间显示
const showSeconds = ref(true);
const now = ref(new Date());
let timer: number | undefined;

// --- 事件监听（Tauri） ---
onMounted(() => {
  console.log('Tauri Vue frontend has loaded.');
  let unlistenMon: UnlistenFn | null = null;
  let unlistenSys: UnlistenFn | null = null;

  (async () => {
    try {
      unlistenMon = await listen<MonitorInfoSnapshot>('monitor-update', (event) => {
        monitorSnapshot.value = event.payload;
      });
      unlistenSys = await listen<SystemSnapshot>('system-update', (event) => {
        systemSnapshot.value = event.payload;
      });
    } catch (e) {
      console.error('Failed to register Tauri event listeners:', e);
    }
  })();

  // 计时器
  startTimer();

  onBeforeUnmount(() => {
    if (unlistenMon) unlistenMon();
    if (unlistenSys) unlistenSys();
    if (timer) clearInterval(timer);
  });
});

watch(showSeconds, () => startTimer());

function startTimer() {
  if (timer) clearInterval(timer);
  timer = window.setInterval(() => {
    now.value = new Date();
  }, showSeconds.value ? 1000 : 60000);
}

// --- 计算属性 ---
const monitorNum = computed(() => monitorSnapshot.value?.monitor_num ?? 0);

const currentSymbol = computed(() => {
  const lts = monitorSnapshot.value?.ltsymbol;
  return parseLtSymbol(lts).symbol;
});

const scaleText = computed(() => {
  const lts = monitorSnapshot.value?.ltsymbol;
  const { scale } = parseLtSymbol(lts);
  return scale !== undefined ? scale.toFixed(2) : '--';
});

// 系统信息颜色等级
const cpuClass = computed(() => {
  if (!systemSnapshot.value) return 'usage-warn';
  const p = systemSnapshot.value.cpu_average;
  return p <= 30 ? 'usage-good' : p <= 60 ? 'usage-warn' : p <= 80 ? 'usage-caution' : 'usage-danger';
});

const memClass = computed(() => {
  if (!systemSnapshot.value) return 'usage-warn';
  const p = systemSnapshot.value.memory_usage_percent;
  return p <= 30 ? 'usage-good' : p <= 60 ? 'usage-warn' : p <= 80 ? 'usage-caution' : 'usage-danger';
});

const battClass = computed(() => {
  if (!systemSnapshot.value) return 'usage-warn';
  const p = systemSnapshot.value.battery_percent;
  return p > 50 ? 'usage-good' : p > 20 ? 'usage-warn' : 'usage-danger';
});

// 时间格式
function pad(n: number) {
  return n.toString().padStart(2, '0');
}

const formattedTime = computed(() => {
  const d = now.value;
  const ts = `${pad(d.getHours())}:${pad(d.getMinutes())}${showSeconds.value ? `:${pad(d.getSeconds())}` : ''}`;
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${ts}`;
});

// --- 事件处理 ---
function buttonClass(i: number) {
  const tagStatus =
    monitorSnapshot.value?.tag_status_vec?.[i] ??
    ({ is_selected: false, is_urg: false, is_filled: false, is_occ: false } as TagStatus);
  const baseClass = getButtonClass(tagStatus);
  const isPressed = pressedButton.value === i;
  return isPressed ? `${baseClass} pressed` : baseClass;
}

async function onTagRelease(index: number) {
  pressedButton.value = null;
  try {
    await invoke('send_tag_command', {
      tagIndex: index,
      isView: true,
      monitorId: monitorNum.value,
    });
  } catch (e) {
    console.error('send_tag_command error:', e);
  }
}

async function onLayoutSelect(idx: number) {
  layoutOpen.value = false;
  try {
    await invoke('send_layout_command', {
      layoutIndex: idx,
      monitorId: monitorNum.value,
    });
  } catch (e) {
    console.error('send_layout_command error:', e);
  }
}

async function onScreenshot() {
  if (isTaking.value) return;
  isTaking.value = true;
  try {
    await invoke('take_screenshot');
  } catch (e) {
    console.error('take_screenshot error:', e);
  } finally {
    setTimeout(() => (isTaking.value = false), 500);
  }
}
</script>

<style>
/* 直接复用你提供的 App.css 内容（略微调整选择器无须改动） */

/* 重置所有默认样式 */
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html,
body {
  margin: 0;
  padding: 0;
  height: 40px !important;
  overflow: hidden;
  font-family:
    system-ui,
    -apple-system,
    BlinkMacSystemFont,
    "Segoe UI",
    Roboto,
    sans-serif;
  background: transparent;
}

#main,
#app {
  margin: 0;
  padding: 0;
  height: 40px !important;
  overflow: hidden;
}

.button-row {
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
  margin: 0;
  padding: 1px 6px;
  gap: 8px;
  width: 100vw;
  height: 40px;
  min-height: 40px;
  max-height: 40px;
  background: rgba(255, 255, 255, 0.95);
  box-shadow: 0 0 10px rgba(0, 0, 0, 0.1);
  position: relative;
  overflow: visible;
  box-sizing: border-box;
}

.buttons-container {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  flex-shrink: 1;
  flex-grow: 0;
  min-width: 0;
  overflow: visible;
  padding: 2px 0; /* 为阴影留出空间 */
}

/* 右侧信息容器 */
.right-info-container {
  display: flex;
  align-items: center;
  gap: 10px; /* 稍微减小gap以容纳更多内容 */
  flex-shrink: 0;
  flex-grow: 0;
  margin-left: auto;
}

/* 系统信息容器 */
.system-info-container {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

/* 单个系统指标 */
.system-metric {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 6px;
  background: rgba(248, 249, 250, 0.8);
  border-radius: 6px;
  border: 1px solid rgba(222, 226, 230, 0.8);
  transition: all 0.2s ease;
  cursor: default;
  user-select: none;
}

.system-metric:hover {
  background: rgba(233, 236, 239, 0.9);
  border-color: rgba(173, 181, 189, 0.8);
  transform: scale(1.02);
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}

/* 指标图标 */
.metric-icon {
  font-size: 14px;
  line-height: 1;
}

/* 指标数值 */
.metric-value {
  font-family:
    "JetBrains Mono", "Fira Code", "Cascadia Code", "SF Mono", Consolas,
    monospace;
  font-size: 13px;
  font-weight: 600;
  min-width: 40px;
  text-align: right;
}

/* 截图按钮样式 */
.screenshot-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  min-width: 40px;
  min-height: 40px;
  background: rgba(248, 249, 250, 0.8);
  border: 1px solid rgba(222, 226, 230, 0.8);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s ease;
  user-select: none;
  flex-shrink: 0;
}

.screenshot-button:hover {
  background: rgba(233, 236, 239, 0.9);
  border-color: rgba(173, 181, 189, 0.8);
  transform: scale(1.05);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.15);
}

.screenshot-button:active {
  transform: scale(0.98);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

/* 截图按钮执行状态 */
.screenshot-button.taking {
  background: linear-gradient(135deg, #007bff, #0056b3);
  border-color: #004085;
  color: white;
  cursor: not-allowed;
}

.screenshot-button.taking:hover {
  transform: none;
  background: linear-gradient(135deg, #007bff, #0056b3);
}

/* 截图图标 */
.screenshot-icon {
  font-size: 20px;
  line-height: 1;
}

/* 时间容器 */
.time-container {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex-shrink: 0;
  flex-grow: 0;
  cursor: pointer;
  transition: all 0.2s ease;
}

.time-container:hover {
  background: rgba(0, 0, 0, 0.05);
  border-radius: 4px;
}

/* 时间显示样式 */
.time-display {
  font-family:
    "JetBrains Mono", "Fira Code", "Cascadia Code", "SF Mono", Consolas,
    monospace;
  font-size: 16px;
  font-weight: 500;
  color: #495057;
  padding: 6px 12px;
  background: rgba(248, 249, 250, 0.8);
  border-radius: 6px;
  border: 1px solid rgba(222, 226, 230, 0.8);
  text-align: center;
  user-select: none;
  transition: all 0.2s ease;
  white-space: nowrap;
  min-width: 65px; /* HH:MM 最小宽度 */
  width: auto !important;
}

.time-display:hover {
  background: rgba(233, 236, 239, 0.9);
  border-color: rgba(173, 181, 189, 0.8);
  transform: scale(1.02);
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}

/* 布局符号样式 */
.layout-symbol {
  color: #000000;
  font-size: 14px;
  padding: 4px 8px;
  background-color: rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  min-width: 20px;
  text-align: center;
  margin-left: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

/* ==================== 按钮基础样式 ==================== */

.emoji-button {
  width: 40px;
  height: 38px;
  min-width: 38;
  min-height: 38;
  max-width: 38;
  max-height: 38;
  font-size: 20px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  cursor: pointer;
  transition: all 0.2s ease;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  user-select: none;
  flex-shrink: 0;
  overflow: hidden;
}

/* 禁用按钮样式 */
.emoji-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  filter: grayscale(50%);
}

/* 确保emoji在效果层之上 */
.emoji-button > * {
  position: relative;
  z-index: 2;
}

/* ==================== 默认状态样式 ==================== */

.emoji-button.state-default {
  background: #ffffff;
  border-color: #dee2e6;
}

.emoji-button.state-default:hover:not(:disabled):not(.pressed):not(:active) {
  background: #f8f9fa;
  border-color: #adb5bd;
  transform: scale(1.02);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.15);
}

/* ==================== 各索引位置的颜色状态 ==================== */

/* 第1个按钮 (索引0) - 红色 #FF6B6B */
.emoji-button:nth-child(1).state-occupied {
  background: rgba(255, 107, 107, 0.3) !important;
  border-color: rgba(255, 107, 107, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(1).state-selected {
  background: rgba(255, 107, 107, 0.7) !important;
  border-color: rgba(255, 107, 107, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(1).state-filtered {
  background: rgba(255, 107, 107, 1) !important;
  border-color: rgba(255, 107, 107, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(255, 107, 107, 0.4);
}

/* 第2个按钮 (索引1) - 青色 #4ECDC4 */
.emoji-button:nth-child(2).state-occupied {
  background: rgba(78, 205, 196, 0.3) !important;
  border-color: rgba(78, 205, 196, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(2).state-selected {
  background: rgba(78, 205, 196, 0.7) !important;
  border-color: rgba(78, 205, 196, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(2).state-filtered {
  background: rgba(78, 205, 196, 1) !important;
  border-color: rgba(78, 205, 196, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(78, 205, 196, 0.4);
}

/* 第3个按钮 (索引2) - 蓝色 #45B7D1 */
.emoji-button:nth-child(3).state-occupied {
  background: rgba(69, 183, 209, 0.3) !important;
  border-color: rgba(69, 183, 209, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(3).state-selected {
  background: rgba(69, 183, 209, 0.7) !important;
  border-color: rgba(69, 183, 209, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(3).state-filtered {
  background: rgba(69, 183, 209, 1) !important;
  border-color: rgba(69, 183, 209, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(69, 183, 209, 0.4);
}

/* 第4个按钮 (索引3) - 绿色 #96CEB4 */
.emoji-button:nth-child(4).state-occupied {
  background: rgba(150, 206, 180, 0.3) !important;
  border-color: rgba(150, 206, 180, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(4).state-selected {
  background: rgba(150, 206, 180, 0.7) !important;
  border-color: rgba(150, 206, 180, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(4).state-filtered {
  background: rgba(150, 206, 180, 1) !important;
  border-color: rgba(150, 206, 180, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(150, 206, 180, 0.4);
}

/* 第5个按钮 (索引4) - 黄色 #FECA57 */
.emoji-button:nth-child(5).state-occupied {
  background: rgba(254, 202, 87, 0.3) !important;
  border-color: rgba(254, 202, 87, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(5).state-selected {
  background: rgba(254, 202, 87, 0.7) !important;
  border-color: rgba(254, 202, 87, 0.9) !important;
  color: #333 !important; /* 黄色背景用深色文字更清晰 */
}

.emoji-button:nth-child(5).state-filtered {
  background: rgba(254, 202, 87, 1) !important;
  border-color: rgba(254, 202, 87, 1) !important;
  color: #333 !important; /* 黄色背景用深色文字更清晰 */
  box-shadow: 0 2px 8px rgba(254, 202, 87, 0.4);
}

/* 第6个按钮 (索引5) - 粉色 #FF9FF3 */
.emoji-button:nth-child(6).state-occupied {
  background: rgba(255, 159, 243, 0.3) !important;
  border-color: rgba(255, 159, 243, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(6).state-selected {
  background: rgba(255, 159, 243, 0.7) !important;
  border-color: rgba(255, 159, 243, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(6).state-filtered {
  background: rgba(255, 159, 243, 1) !important;
  border-color: rgba(255, 159, 243, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(255, 159, 243, 0.4);
}

/* 第7个按钮 (索引6) - 淡蓝色 #54A0FF */
.emoji-button:nth-child(7).state-occupied {
  background: rgba(84, 160, 255, 0.3) !important;
  border-color: rgba(84, 160, 255, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(7).state-selected {
  background: rgba(84, 160, 255, 0.7) !important;
  border-color: rgba(84, 160, 255, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(7).state-filtered {
  background: rgba(84, 160, 255, 1) !important;
  border-color: rgba(84, 160, 255, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(84, 160, 255, 0.4);
}

/* 第8个按钮 (索引7) - 紫色 #5F27CD */
.emoji-button:nth-child(8).state-occupied {
  background: rgba(95, 39, 205, 0.3) !important;
  border-color: rgba(95, 39, 205, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(8).state-selected {
  background: rgba(95, 39, 205, 0.7) !important;
  border-color: rgba(95, 39, 205, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(8).state-filtered {
  background: rgba(95, 39, 205, 1) !important;
  border-color: rgba(95, 39, 205, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(95, 39, 205, 0.4);
}

/* 第9个按钮 (索引8) - 青绿色 #00D2D3 */
.emoji-button:nth-child(9).state-occupied {
  background: rgba(0, 210, 211, 0.3) !important;
  border-color: rgba(0, 210, 211, 0.6) !important;
  color: #333 !important;
}

.emoji-button:nth-child(9).state-selected {
  background: rgba(0, 210, 211, 0.7) !important;
  border-color: rgba(0, 210, 211, 0.9) !important;
  color: white !important;
}

.emoji-button:nth-child(9).state-filtered {
  background: rgba(0, 210, 211, 1) !important;
  border-color: rgba(0, 210, 211, 1) !important;
  color: white !important;
  box-shadow: 0 2px 8px rgba(0, 210, 211, 0.4);
}

/* ==================== Urgent 状态保持原有样式 ==================== */
.emoji-button.state-urgent {
  background: linear-gradient(135deg, #dc3545, #c82333) !important;
  border-color: #bd2130 !important;
  color: white !important;
}

.emoji-button.state-urgent::after {
  content: "U";
  position: absolute;
  top: -3px;
  right: -3px;
  background: #ffc107;
  border-radius: 50%;
  width: 12px;
  height: 12px;
  border: 1px solid white;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  font-size: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #000;
  font-weight: bold;
}

/* ==================== 状态指示器优化 ==================== */

/* Filtered状态指示器 */
.emoji-button.state-filtered::after {
  content: "●";
  position: absolute;
  top: 2px;
  right: 2px;
  color: rgba(255, 255, 255, 0.9);
  font-size: 10px;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
  font-weight: bold;
}

/* Selected状态指示器 */
.emoji-button.state-selected::after {
  content: "◆";
  position: absolute;
  top: 2px;
  right: 2px;
  color: rgba(255, 255, 255, 0.9);
  font-size: 8px;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
  font-weight: bold;
}

/* 黄色按钮的特殊处理 */
.emoji-button:nth-child(5).state-selected::after,
.emoji-button:nth-child(5).state-filtered::after {
  color: rgba(51, 51, 51, 0.8);
  text-shadow: 0 1px 1px rgba(255, 255, 255, 0.3);
}

/* Occupied状态指示器 */
.emoji-button.state-occupied::after {
  content: "○";
  position: absolute;
  top: 2px;
  right: 2px;
  color: rgba(51, 51, 51, 0.7);
  font-size: 8px;
  text-shadow: 0 1px 1px rgba(255, 255, 255, 0.3);
  font-weight: bold;
}

/* ==================== 动画效果 ==================== */

@keyframes filtered-glow {
  0%,
  100% {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
    transform: scale(1);
  }
  50% {
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    transform: scale(1.02);
  }
}

@keyframes selected-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.9;
    transform: scale(1.01);
  }
}

@keyframes urgent-blink {
  0%,
  50%,
  100% {
    opacity: 1;
    box-shadow: 0 1px 3px rgba(220, 53, 69, 0.5);
  }
  25%,
  75% {
    opacity: 0.9;
    box-shadow: 0 2px 8px rgba(220, 53, 69, 0.8);
    transform: scale(1.04);
  }
}

/* ==================== 按下效果 ==================== */

/* 按下波纹效果基础 */
.emoji-button::before {
  content: "";
  position: absolute;
  top: 50%;
  left: 50%;
  width: 0;
  height: 0;
  border-radius: 50%;
  background: radial-gradient(
    circle,
    rgba(255, 255, 255, 0.6) 0%,
    rgba(255, 255, 255, 0) 70%
  );
  transform: translate(-50%, -50%);
  opacity: 0;
  pointer-events: none;
  z-index: 1;
  transition: all 0.3s ease;
}

.emoji-button.pressed::before,

/* 基础按钮按下效果 */
.emoji-button.pressed,
.emoji-button:active {
  transform: scale(0.92) !important;
  box-shadow:
    inset 0 2px 6px rgba(0, 0, 0, 0.3),
    0 1px 2px rgba(0, 0, 0, 0.2) !important;
  transition: all 0.1s ease !important;
}

/* 各状态按下效果 */
.emoji-button.state-default.pressed,
.emoji-button.state-default:active {
  background: #dee2e6 !important;
  border-color: #6c757d !important;
}

.emoji-button.state-occupied.pressed,
.emoji-button.state-selected.pressed,
.emoji-button.state-filtered.pressed {
  opacity: 0.8;
  transform: scale(0.92) !important;
  box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.3) !important;
}

/* ==================== 悬停效果（仅默认状态） ==================== */

.emoji-button:nth-child(1):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #ff6b6b !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(255, 107, 107, 0.4);
  background: rgba(255, 107, 107, 0.1);
}

.emoji-button:nth-child(2):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #4ecdc4 !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(78, 205, 196, 0.4);
  background: rgba(78, 205, 196, 0.1);
}

.emoji-button:nth-child(3):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #45b7d1 !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(69, 183, 209, 0.4);
  background: rgba(69, 183, 209, 0.1);
}

.emoji-button:nth-child(4):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #96ceb4 !important;
  border-width: 2px !重要;
  box-shadow: 0 0 6px rgba(150, 206, 180, 0.4);
  background: rgba(150, 206, 180, 0.1);
}

.emoji-button:nth-child(5):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #feca57 !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(254, 202, 87, 0.4);
  background: rgba(254, 202, 87, 0.1);
}

.emoji-button:nth-child(6):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #ff9ff3 !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(255, 159, 243, 0.4);
  background: rgba(255, 159, 243, 0.1);
}

.emoji-button:nth-child(7):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #54a0ff !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(84, 160, 255, 0.4);
  background: rgba(84, 160, 255, 0.1);
}

.emoji-button:nth-child(8):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #5f27cd !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(95, 39, 205, 0.4);
  background: rgba(95, 39, 205, 0.1);
}

.emoji-button:nth-child(9):hover:not(:disabled):not(.pressed):not(:active):not(
    .state-occupied
  ):not(.state-selected):not(.state-filtered):not(.state-urgent) {
  border-color: #00d2d3 !important;
  border-width: 2px !important;
  box-shadow: 0 0 6px rgba(0, 210, 211, 0.4);
  background: rgba(0, 210, 211, 0.1);
}

/* 优化过渡效果 */
.emoji-button {
  transition:
    all 0.2s ease,
    border-color 0.15s ease,
    box-shadow 0.15s ease;
}

/* 正常悬停效果 - 不与按下效果冲突 */
.emoji-button:hover:not(.pressed):not(:active) {
  transform: scale(1.05);
  transition: all 0.2s ease;
}

/* ==================== 禁用状态优化 ==================== */

.emoji-button:disabled.pressed,
.emoji-button:disabled:active {
  transform: none !important;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1) !important;
  background: #f8f9fa !important;
}

.emoji-button:disabled::before {
  display: none;
}

/* 触摸设备优化 */
@media (hover: none) {
  .emoji-button:hover {
    transform: none;
  }

  .emoji-button.pressed,
  .emoji-button:active {
    transform: scale(0.95) !important;
  }
}

/* ========= 新增：通用 pill 样式 ========= */
.pill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  padding: 4px 10px;
  font-size: 14px;
  line-height: 1;
  border: 1px solid transparent;
  transition: all 120ms ease-in-out;
  white-space: nowrap;
}

/* ========= 新增：系统指标 pill 颜色等级 ========= */
.system-info-container {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.usage-pill {
  color: #fff;
  border-width: 1px;
}

.usage-good {
  background: rgba(31, 191, 81, 0.90);
  border-color: #1fbf51;
}
.usage-warn {
  background: rgba(244, 194, 13, 0.90);
  border-color: #f4c20d;
  color: #000;
}
.usage-caution {
  background: rgba(255, 140, 26, 0.90);
  border-color: #ff8c1a;
}
.usage-danger {
  background: rgba(229, 57, 53, 0.90);
  border-color: #e53935;
}

/* ========= 新增：布局开关与选项 ========= */
.layout-controls {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-left: 6px;
}

.layout-toggle {
  cursor: pointer;
  color: #fff;
}

.layout-toggle.open {
  background: rgba(60, 179, 113, 0.85); /* 绿色 */
  border-color: #3cb371;
}
.layout-toggle.closed {
  background: rgba(211, 84, 0, 0.85); /* 橙色 */
  border-color: #d35400;
}
.layout-toggle:hover {
  filter: brightness(1.05);
  border-width: 2px;
}

.layout-selector {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.layout-option {
  cursor: pointer;
  color: #fff;
  background: rgba(65, 105, 225, 0.85); /* royal blue */
  border-color: #4169e1;
}
.layout-option.current {
  background: rgba(60, 179, 113, 0.9); /* open 绿 */
  border-color: #3cb371;
  border-width: 2px;
}
.layout-option:hover {
  filter: brightness(1.05);
  border-width: 2px;
}

/* ========= 新增：截图/时间/显示器/缩放 pill ========= */
.screenshot-pill {
  cursor: pointer;
  color: #fff;
  background: rgba(0, 204, 204, 0.9);
  border-color: #00cccc;
}
.screenshot-pill:hover {
  background: rgba(255, 136, 0, 0.95);
  border-color: #ff8800;
}

.time-pill {
  color: #fff;
  background: rgba(77, 163, 255, 0.90);
  border-color: #4da3ff;
  cursor: pointer;
}

.monitor-pill {
  color: #fff;
  background: rgba(155, 89, 182, 0.90);
  border-color: #9b59b6;
}

.scale-pill {
  color: #fff;
  background: rgba(120, 120, 120, 0.88);
  border-color: #777;
}

/* 中间撑开（如果没有的话新增） */
.spacer {
  flex: 1 1 auto;
}
</style>
