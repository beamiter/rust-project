// src/backend/wayland/input_ops.rs
use super::event_source::CompositorCommand;
use crate::backend::api::{AllowMode, InputOps, WindowId};
// FIX 8: Use the Sender from calloop to match the event source
use smithay::reexports::calloop::channel::Sender as CommandSender;
use crate::backend::common_define::StdCursorKind;

#[derive(Clone)]
pub struct PointerController;
impl PointerController {
    pub fn new() -> Self {
        Self
    }
}

pub struct WaylandInputOps {
    command_tx: CommandSender<CompositorCommand>,
}

impl WaylandInputOps {
    pub fn new(command_tx: CommandSender<CompositorCommand>) -> Self {
        Self { command_tx }
    }
}

impl InputOps for WaylandInputOps {
    fn set_cursor(&self, kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        let name = super::cursor::WaylandCursorProvider::map_kind(kind);
        self.command_tx
            .send(CompositorCommand::SetCursor(name.to_string()))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    fn grab_pointer(
        &self,
        _mask: u32,
        _cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }

    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn allow_events(&self, _mode: AllowMode, _time: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        // TODO: 可通过 command/response channel 从 JwmWlState 获取真实位置
        Ok((0, 0, 0, 0))
    }

    fn warp_pointer_to_window(
        &self,
        _win: WindowId,
        _x: i16,
        _y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("Wayland does not support pointer warping".into())
    }

    fn drag_loop(
        &self,
        _cursor: Option<u64>,
        _warp_to: Option<(i16, i16)>,
        target: WindowId,
        _on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 发送一个异步命令来启动拖拽
        self.command_tx
            .send(CompositorCommand::StartMoveGrab(target))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}
