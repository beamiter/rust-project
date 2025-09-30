import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

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

interface UiState {
  monitor_info_snapshot: MonitorInfoSnapshot | null;
  system_snapshot: SystemSnapshot | null;
}

// --- 帮助函数 ---
const BUTTONS = ["🐖", "🐄", "🐂", "🐃", "🦥", "🦣", "🐏", "🦆", "🐢"];

const getButtonClass = (tagStatus: TagStatus): string => {
  if (tagStatus.is_filled) return "emoji-button state-filtered";
  if (tagStatus.is_selected) return "emoji-button state-selected";
  if (tagStatus.is_urg) return "emoji-button state-urgent";
  if (tagStatus.is_occ) return "emoji-button state-occupied";
  return "emoji-button state-default";
};

const formatBytes = (bytes: number): string => {
  if (bytes === 0) return "0B";
  const UNITS = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const size = parseFloat((bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1));
  return `${size}${UNITS[i]}`;
};

// 解析 ltsymbol：提取布局符号与缩放因子
function parseLtSymbol(lts: string | undefined) {
  if (!lts) return { symbol: "[]=", scale: undefined };
  const symbolMatch = lts.match(/^(\S+)/);
  const scaleMatch = lts.match(/s:\s*([0-9.]+)/i);
  const symbol = symbolMatch ? symbolMatch[1] : "[]=";
  const scale = scaleMatch ? parseFloat(scaleMatch[1]) : undefined;
  return { symbol, scale };
}

function monitorIcon(num: number) {
  // Nerd Font 字体存在时会显示图标，否则你可以改为 `M${num}`
  if (num === 0) return "󰎡";
  if (num === 1) return "󰎤";
  return `M${num}`;
}

// --- 子组件 ---

const TagButtons = (
  { tags, monitorNum }: { tags: TagStatus[]; monitorNum: number },
) => {
  const [pressedButton, setPressedButton] = useState<number | null>(null);

  const handlePress = (index: number) => {
    setPressedButton(index);
  };

  const handleRelease = (index: number) => {
    setPressedButton(null);
    invoke("send_tag_command", {
      tagIndex: index,
      isView: true,
      monitorId: monitorNum,
    }).catch((e) => console.error(e));
  };

  return (
    <>
      {BUTTONS.map((emoji, i) => {
        const tagStatus = tags[i] || {
          is_selected: false,
          is_urg: false,
          is_filled: false,
          is_occ: false,
        };
        const baseClass = getButtonClass(tagStatus);
        const isPressed = pressedButton === i;
        const buttonClass = isPressed ? `${baseClass} pressed` : baseClass;

        return (
          <button
            key={i}
            className={buttonClass}
            onMouseDown={() => handlePress(i)}
            onMouseUp={() => handleRelease(i)}
            onMouseLeave={() => setPressedButton(null)}
          >
            {emoji}
          </button>
        );
      })}
    </>
  );
};

const SystemInfoDisplay = ({ snapshot }: { snapshot: SystemSnapshot | null }) => {
  if (!snapshot) {
    return (
      <div className="system-info-container">
        <div className="pill usage-pill usage-warn">CPU --%</div>
        <div className="pill usage-pill usage-warn">MEM --%</div>
        <div className="pill usage-pill usage-warn">🔋 --%</div>
      </div>
    );
  }

  const sev = (p: number) =>
    p <= 30 ? "usage-good" : p <= 60 ? "usage-warn" : p <= 80 ? "usage-caution" : "usage-danger";

  const cpuClass = sev(snapshot.cpu_average);
  const memClass = sev(snapshot.memory_usage_percent);
  const battClass = snapshot.battery_percent > 50
    ? "usage-good"
    : snapshot.battery_percent > 20
    ? "usage-warn"
    : "usage-danger";
  const batteryIcon = snapshot.is_charging ? "🔌" : "🔋";

  return (
    <div className="system-info-container">
      <div className={`pill usage-pill ${cpuClass}`} title="CPU 平均使用率">
        {`CPU ${snapshot.cpu_average.toFixed(0)}%`}
      </div>
      <div
        className={`pill usage-pill ${memClass}`}
        title={`内存使用: ${formatBytes(snapshot.memory_used)} / ${formatBytes(snapshot.memory_total)}`}
      >
        {`MEM ${snapshot.memory_usage_percent.toFixed(0)}%`}
      </div>
      <div
        className={`pill usage-pill ${battClass}`}
        title={
          snapshot.is_charging
            ? `电池充电中: ${snapshot.battery_percent.toFixed(1)}%`
            : `电池电量: ${snapshot.battery_percent.toFixed(1)}%`
        }
      >
        {`${batteryIcon} ${snapshot.battery_percent.toFixed(0)}%`}
      </div>
    </div>
  );
};

const ScreenshotButton = () => {
  const [isTaking, setIsTaking] = useState(false);

  const handleClick = async () => {
    if (isTaking) return;
    setIsTaking(true);
    try {
      await invoke("take_screenshot");
    } catch (e) {
      console.error(e);
    } finally {
      setTimeout(() => setIsTaking(false), 500);
    }
  };

  return (
    <div
      className={`pill screenshot-pill ${isTaking ? "taking" : ""}`}
      onClick={handleClick}
      title="截图 (Flameshot)"
    >
      {isTaking ? "⏳" : "📸"}
    </div>
  );
};

const TimeDisplay = () => {
  const [showSeconds, setShowSeconds] = useState(true);
  const [time, setTime] = useState(new Date());

  useEffect(() => {
    const interval = setInterval(() => setTime(new Date()), showSeconds ? 1000 : 60000);
    return () => clearInterval(interval);
  }, [showSeconds]);

  const pad = (n: number) => n.toString().padStart(2, "0");
  const formatted = useMemo(() => {
    const d = time;
    const ts = `${pad(d.getHours())}:${pad(d.getMinutes())}${
      showSeconds ? `:${pad(d.getSeconds())}` : ""
    }`;
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${ts}`;
  }, [time, showSeconds]);

  return (
    <div className="pill time-pill" onClick={() => setShowSeconds(!showSeconds)} title="点击切换秒显示">
      {formatted}
    </div>
  );
};

const LayoutControls = ({
  ltsymbol,
  monitorNum,
}: {
  ltsymbol: string;
  monitorNum: number;
}) => {
  const [open, setOpen] = useState(false);
  const { symbol } = parseLtSymbol(ltsymbol);

  const toggleClass = `pill layout-toggle ${open ? "open" : "closed"}`;
  const optClass = (sym: string) =>
    `pill layout-option ${symbol === sym ? "current" : ""}`;

  const onSelect = (idx: number) => {
    setOpen(false);
    invoke("send_layout_command", {
      layoutIndex: idx,
      monitorId: monitorNum,
    }).catch((e) => console.error(e));
  };

  return (
    <div className="layout-controls">
      <div className={toggleClass} onClick={() => setOpen(!open)} title="切换布局">
        {symbol}
      </div>
      {open && (
        <div className="layout-selector">
          <div className={optClass("[]=")} onClick={() => onSelect(0)}>
            []=
          </div>
          <div className={optClass("><>")} onClick={() => onSelect(1)}>
            <>{"><>"}</>
          </div>
          <div className={optClass("[M]")} onClick={() => onSelect(2)}>
            [M]
          </div>
        </div>
      )}
    </div>
  );
};

// --- 主 App 组件 ---
function App() {
  const [appState, setAppState] = useState<UiState>({
    monitor_info_snapshot: null,
    system_snapshot: null,
  });

  useEffect(() => {
    console.log("Tauri React frontend has loaded.");

    const unlistenMonitor = listen<MonitorInfoSnapshot>("monitor-update", (event) => {
      setAppState((prev) => ({ ...prev, monitor_info_snapshot: event.payload }));
    });

    const unlistenSystem = listen<SystemSnapshot>("system-update", (event) => {
      setAppState((prev) => ({ ...prev, system_snapshot: event.payload }));
    });

    return () => {
      unlistenMonitor.then((f) => f());
      unlistenSystem.then((f) => f());
    };
  }, []);

  if (!appState.monitor_info_snapshot) {
    return <div className="button-row">Loading...</div>;
  }

  const mis = appState.monitor_info_snapshot;
  const { scale } = parseLtSymbol(mis.ltsymbol);

  return (
    <div className="button-row">
      <div className="buttons-container">
        <TagButtons tags={mis.tag_status_vec} monitorNum={mis.monitor_num} />
        <LayoutControls ltsymbol={mis.ltsymbol} monitorNum={mis.monitor_num} />
      </div>

      <div className="spacer" />

      <div className="right-info-container">
        <SystemInfoDisplay snapshot={appState.system_snapshot} />
        <ScreenshotButton />
        <TimeDisplay />
        <div className="pill monitor-pill" title="显示器">
          {"🖥️ " + monitorIcon(mis.monitor_num)}
        </div>
        <div className="pill scale-pill" title="Scale Factor">
          {scale !== undefined ? `s: ${scale.toFixed(2)}` : "s: --"}
        </div>
      </div>
    </div>
  );
}

export default App;
