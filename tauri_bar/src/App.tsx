import { useState, useEffect} from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import './App.css'; // 导入我们的样式

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
  ltsymbol: string;
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
  monitor_info: MonitorInfoSnapshot;
  system_snapshot: SystemSnapshot | null;
}

// --- 帮助函数 (从 Rust 移植到 JS) ---
const BUTTONS = ["🐖", "🐄", "🐂", "🐃", "🦥", "🦣", "🐏", "🦆", "🐢"];

const getButtonClass = (tagStatus: TagStatus): string => {
  if (tagStatus.is_filled) return "emoji-button state-filtered";
  if (tagStatus.is_selected) return "emoji-button state-selected";
  if (tagStatus.is_urg) return "emoji-button state-urgent";
  if (tagStatus.is_occ) return "emoji-button state-occupied";
  return "emoji-button state-default";
};

const formatBytes = (bytes: number): string => {
  if (bytes === 0) return '0B';
  const UNITS = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const size = parseFloat((bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1));
  return `${size}${UNITS[i]}`;
};

// --- 子组件 ---

const TagButtons = ({ tags, monitorNum }: { tags: TagStatus[], monitorNum: number }) => {
  const [pressedButton, setPressedButton] = useState<number | null>(null);

  const handlePress = (index: number) => {
    setPressedButton(index);
  };

  const handleRelease = (index: number) => {
    setPressedButton(null);
    invoke('send_tag_command', { tagIndex: index, isView: true, monitorId: monitorNum });
  };

  return (
    <>
      {BUTTONS.map((emoji, i) => {
        const tagStatus = tags[i] || { is_selected: false, is_urg: false, is_filled: false, is_occ: false };
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
    // 渲染占位符
    return (
      <div className="system-info-container">
        {/* ... 省略占位符 JSX ... */}
      </div>
    );
  }

  const getCpuColor = (avg: number) => avg > 80 ? '#dc3545' : avg > 60 ? '#ffc107' : '#28a745';
  const getMemColor = (perc: number) => perc > 85 ? '#dc3545' : perc > 70 ? '#ffc107' : '#28a745';
  const getBatteryColor = (perc: number) => perc > 50 ? '#28a745' : perc > 20 ? '#ffc107' : '#dc3545';
  const getBatteryIcon = (perc: number, isCharging: boolean) => {
    if (isCharging) return "🔌";
    if (perc > 75) return "🔋";
    return "🪫";
  };

  return (
    <div className="system-info-container">
      <div className="system-metric" title="CPU 平均使用率">
        <span className="metric-icon">💻</span>
        <span className="metric-value" style={{ color: getCpuColor(snapshot.cpu_average) }}>
          {snapshot.cpu_average.toFixed(1)}%
        </span>
      </div>
      <div className="system-metric" title={`内存使用: ${formatBytes(snapshot.memory_used)} / ${formatBytes(snapshot.memory_total)}`}>
        <span className="metric-icon">🧠</span>
        <span className="metric-value" style={{ color: getMemColor(snapshot.memory_usage_percent) }}>
          {snapshot.memory_usage_percent.toFixed(1)}%
        </span>
      </div>
      <div className="system-metric" title={snapshot.is_charging ? `电池充电中: ${snapshot.battery_percent.toFixed(1)}%` : `电池电量: ${snapshot.battery_percent.toFixed(1)}%`}>
        <span className="metric-icon">{getBatteryIcon(snapshot.battery_percent, snapshot.is_charging)}</span>
        <span className="metric-value" style={{ color: getBatteryColor(snapshot.battery_percent) }}>
          {snapshot.battery_percent.toFixed(0)}%
        </span>
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
      await invoke('take_screenshot');
    } catch (e) {
      console.error(e);
    } finally {
      // 添加一个短暂延迟以改善用户体验
      setTimeout(() => setIsTaking(false), 500);
    }
  };

  const buttonClass = isTaking ? 'screenshot-button taking' : 'screenshot-button';

  return (
    <button className={buttonClass} onClick={handleClick} title="截图 (Flameshot)" disabled={isTaking}>
      <span className="screenshot-icon">{isTaking ? '⏳' : '📷'}</span>
    </button>
  );
};


const TimeDisplay = () => {
  const [showSeconds, setShowSeconds] = useState(true);
  const [time, setTime] = useState(new Date());

  useEffect(() => {
    const interval = setInterval(() => {
      setTime(new Date());
    }, showSeconds ? 1000 : 60000);
    return () => clearInterval(interval);
  }, [showSeconds]);

  const format = (d: Date) => {
    const pad = (n: number) => n.toString().padStart(2, '0');
    const timeStr = `${pad(d.getHours())}:${pad(d.getMinutes())}${showSeconds ? `:${pad(d.getSeconds())}` : ''}`;
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${timeStr}`;
  }

  return (
    <div className="time-container" onClick={() => setShowSeconds(!showSeconds)}>
      <div className="time-display">{format(time)}</div>
    </div>
  );
};


// --- 主 App 组件 ---

function App() {
  const [uiState, setUiState] = useState<UiState | null>(null);

  useEffect(() => {
    // 启动时立即打印日志，确认前端已加载
    console.log("Tauri React frontend has loaded.");

    const unlisten = listen<UiState>('state-update', (event) => {
      setUiState(event.payload);
    });

    // 组件卸载时清理监听器
    return () => {
      unlisten.then(f => f());
    };
  }, []);

  if (!uiState) {
    return <div className="button-row">Loading...</div>; // 或者一个更精美的加载界面
  }

  const { monitor_info, system_snapshot } = uiState;

  return (
    <div className="button-row">
      <div className="buttons-container">
        <TagButtons tags={monitor_info.tag_status_vec} monitorNum={monitor_info.monitor_num} />
        <span className="layout-symbol" title="当前布局">
          {monitor_info.ltsymbol}
        </span>
      </div>

      <div className="right-info-container">
        <SystemInfoDisplay snapshot={system_snapshot} />
        <ScreenshotButton />
        <TimeDisplay />
      </div>
    </div>
  );
}

export default App;
