use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::layout::LayoutEnum;
use crate::jwm::{Jwm, WMArgEnum, WMFuncType};

// ---------------------------------------------------------------------------
// Wire protocol types (newline-delimited JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum IpcMessage {
    Command(IpcCommand),
    Query(IpcQuery),
    Subscribe(IpcSubscribe),
}

#[derive(Debug, Deserialize)]
pub struct IpcCommand {
    pub command: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Deserialize)]
pub struct IpcQuery {
    pub query: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Deserialize)]
pub struct IpcSubscribe {
    pub subscribe: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct IpcResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn ok(data: Option<Value>) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IpcEvent {
    pub event: String,
    pub payload: Value,
}

// ---------------------------------------------------------------------------
// Query result types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct WindowInfo {
    pub id: u64,
    pub name: String,
    pub class: String,
    pub instance: String,
    pub tags: u32,
    pub monitor: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub is_floating: bool,
    pub is_fullscreen: bool,
    pub is_urgent: bool,
    pub is_sticky: bool,
    pub is_focused: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceInfo {
    pub tag_mask: u32,
    pub tag_index: usize,
    pub monitor: i32,
    pub layout: String,
    pub m_fact: f32,
    pub n_master: u32,
    pub num_clients: usize,
    pub focused: bool,
}

#[derive(Debug, Serialize)]
pub struct MonitorInfoIpc {
    pub num: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub active_tags: u32,
    pub layout: String,
    pub focused: bool,
}

#[derive(Debug, Serialize)]
pub struct TreeNode {
    pub monitor: MonitorInfoIpc,
    pub windows: Vec<WindowInfo>,
}

// ---------------------------------------------------------------------------
// Command dispatch — maps command name → (WMFuncType, WMArgEnum)
// ---------------------------------------------------------------------------

pub fn dispatch_command(name: &str, args: &Value) -> Result<(WMFuncType, WMArgEnum), String> {
    match name {
        // --- Window management ---
        "focusstack" => Ok((Jwm::focusstack as WMFuncType, parse_int_arg(args, 1))),
        "killclient" => Ok((Jwm::killclient, parse_int_arg(args, 0))),
        "zoom" => Ok((Jwm::zoom, parse_int_arg(args, 0))),
        "togglefloating" => Ok((Jwm::togglefloating, parse_int_arg(args, 0))),
        "togglesticky" => Ok((Jwm::togglesticky, parse_int_arg(args, 0))),
        "togglepip" => Ok((Jwm::togglepip, parse_int_arg(args, 0))),
        "togglescratchpad" => {
            let cmd = parse_string_vec_arg(args)
                .unwrap_or_else(|_| vec!["term".to_string()]);
            Ok((Jwm::togglescratchpad, WMArgEnum::StringVec(cmd)))
        }
        "movestack" => Ok((Jwm::movestack, parse_int_arg(args, 1))),

        // --- Layout ---
        "setmfact" => Ok((Jwm::setmfact, parse_float_arg(args, 0.0))),
        "setcfact" => Ok((Jwm::setcfact, parse_float_arg(args, 0.0))),
        "incnmaster" => Ok((Jwm::incnmaster, parse_int_arg(args, 1))),
        "setlayout" => {
            let layout = parse_layout_arg(args)?;
            Ok((Jwm::setlayout, layout))
        }
        "cyclelayout" => Ok((Jwm::cyclelayout, parse_int_arg(args, 1))),
        "togglebar" => Ok((Jwm::togglebar, parse_int_arg(args, 0))),

        // --- Tags ---
        "view" => Ok((Jwm::view, parse_uint_arg(args))),
        "tag" => Ok((Jwm::tag, parse_uint_arg(args))),
        "toggleview" => Ok((Jwm::toggleview, parse_uint_arg(args))),
        "toggletag" => Ok((Jwm::toggletag, parse_uint_arg(args))),
        "loopview" => Ok((Jwm::loopview, parse_int_arg(args, 1))),

        // --- Monitor ---
        "focusmon" => Ok((Jwm::focusmon, parse_int_arg(args, 1))),
        "tagmon" => Ok((Jwm::tagmon, parse_int_arg(args, 1))),

        // --- Spawn ---
        "spawn" => {
            let cmd = parse_string_vec_arg(args)?;
            Ok((Jwm::spawn, WMArgEnum::StringVec(cmd)))
        }

        // --- Misc ---
        "quit" => Ok((Jwm::quit, parse_int_arg(args, 0))),
        "restart" => Ok((Jwm::restart, parse_int_arg(args, 0))),

        _ => Err(format!("unknown command: {name}")),
    }
}

// ---------------------------------------------------------------------------
// Argument parsers
// ---------------------------------------------------------------------------

fn parse_int_arg(args: &Value, default: i32) -> WMArgEnum {
    let v = args
        .get("value")
        .or_else(|| args.get("v"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .or_else(|| args.as_i64().map(|v| v as i32))
        .unwrap_or(default);
    WMArgEnum::Int(v)
}

fn parse_float_arg(args: &Value, default: f32) -> WMArgEnum {
    let v = args
        .get("value")
        .or_else(|| args.get("v"))
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .or_else(|| args.as_f64().map(|v| v as f32))
        .unwrap_or(default);
    WMArgEnum::Float(v)
}

fn parse_uint_arg(args: &Value) -> WMArgEnum {
    let v = args
        .get("tag")
        .or_else(|| args.get("value"))
        .or_else(|| args.get("v"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| args.as_u64().map(|v| v as u32))
        .unwrap_or(0);
    WMArgEnum::UInt(v)
}

fn parse_string_vec_arg(args: &Value) -> Result<Vec<String>, String> {
    if let Some(arr) = args.get("cmd").and_then(|v| v.as_array()) {
        Ok(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
    } else if let Some(arr) = args.as_array() {
        Ok(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
    } else if let Some(s) = args.as_str() {
        Ok(vec![s.to_string()])
    } else {
        Err("spawn requires a command array or string".into())
    }
}

fn parse_layout_arg(args: &Value) -> Result<WMArgEnum, String> {
    let name = args
        .get("layout")
        .or_else(|| args.get("value"))
        .and_then(|v| v.as_str())
        .or_else(|| args.as_str())
        .unwrap_or("tile");
    let layout = match name.to_lowercase().as_str() {
        "tile" => LayoutEnum::TILE,
        "float" => LayoutEnum::FLOAT,
        "monocle" => LayoutEnum::MONOCLE,
        "fibonacci" => LayoutEnum::FIBONACCI,
        "centered_master" | "centeredmaster" => LayoutEnum::CENTERED_MASTER,
        "bstack" => LayoutEnum::BSTACK,
        "grid" => LayoutEnum::GRID,
        "deck" => LayoutEnum::DECK,
        "three_col" | "threecol" => LayoutEnum::THREE_COL,
        "tatami" => LayoutEnum::TATAMI,
        "fullscreen" => LayoutEnum::FULLSCREEN,
        _ => return Err(format!("unknown layout: {name}")),
    };
    Ok(WMArgEnum::Layout(std::rc::Rc::new(layout)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_message() {
        let json = r#"{"command": "view", "args": {"tag": 2}}"#;
        let msg: IpcMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, IpcMessage::Command(IpcCommand { command, .. }) if command == "view"));
    }

    #[test]
    fn parse_query_message() {
        let json = r#"{"query": "get_windows"}"#;
        let msg: IpcMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, IpcMessage::Query(IpcQuery { query, .. }) if query == "get_windows"));
    }

    #[test]
    fn parse_subscribe_message() {
        let json = r#"{"subscribe": ["window", "tag"]}"#;
        let msg: IpcMessage = serde_json::from_str(json).unwrap();
        match msg {
            IpcMessage::Subscribe(sub) => {
                assert_eq!(sub.subscribe, vec!["window", "tag"]);
            }
            _ => panic!("expected Subscribe"),
        }
    }

    #[test]
    fn dispatch_known_commands() {
        // view
        let args = serde_json::json!({"tag": 4});
        let (_, arg) = dispatch_command("view", &args).unwrap();
        assert_eq!(arg, WMArgEnum::UInt(4));

        // focusstack
        let args = serde_json::json!({"value": -1});
        let (_, arg) = dispatch_command("focusstack", &args).unwrap();
        assert_eq!(arg, WMArgEnum::Int(-1));

        // setmfact
        let args = serde_json::json!(0.05);
        let (_, arg) = dispatch_command("setmfact", &args).unwrap();
        assert_eq!(arg, WMArgEnum::Float(0.05));

        // killclient (no args)
        let args = serde_json::json!(null);
        let (_, arg) = dispatch_command("killclient", &args).unwrap();
        assert_eq!(arg, WMArgEnum::Int(0));
    }

    #[test]
    fn dispatch_unknown_command_errors() {
        let args = serde_json::json!(null);
        assert!(dispatch_command("nonexistent", &args).is_err());
    }

    #[test]
    fn dispatch_spawn_command() {
        let args = serde_json::json!({"cmd": ["alacritty", "--title", "test"]});
        let (_, arg) = dispatch_command("spawn", &args).unwrap();
        assert_eq!(
            arg,
            WMArgEnum::StringVec(vec![
                "alacritty".into(),
                "--title".into(),
                "test".into()
            ])
        );
    }

    #[test]
    fn dispatch_layout_command() {
        let args = serde_json::json!({"layout": "monocle"});
        let result = dispatch_command("setlayout", &args);
        assert!(result.is_ok());
        let (_, arg) = result.unwrap();
        assert!(matches!(arg, WMArgEnum::Layout(_)));
    }

    #[test]
    fn response_serialization() {
        let ok = IpcResponse::ok(Some(serde_json::json!({"version": "0.2.0"})));
        let json = serde_json::to_string(&ok).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"version\""));
        assert!(!json.contains("\"error\""));

        let err = IpcResponse::err("bad command");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("bad command"));
    }
}
