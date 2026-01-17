// src/backend/x11/ewmh_facade.rs
use crate::backend::api::{EwmhFacade, EwmhFeature};
use crate::backend::common_define::WindowId;
use crate::backend::error::BackendError;
use crate::backend::x11::Atoms;
use crate::backend::x11::ids::X11IdRegistry;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::CreateWindowAux;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xproto::{AtomEnum, PropMode};
use x11rb::wrapper::ConnectionExt as _;

pub struct X11EwmhFacade<C: Connection> {
    conn: Arc<C>,
    root: WindowId,
    atoms: Atoms,
    ids: X11IdRegistry,
}

impl<C: Connection + Send + Sync + 'static> X11EwmhFacade<C> {
    pub fn new(conn: Arc<C>, root: WindowId, atoms: Atoms, ids: X11IdRegistry) -> Self {
        Self {
            conn,
            root,
            atoms,
            ids,
        }
    }
    fn feature_to_atom(&self, f: EwmhFeature) -> u32 {
        match f {
            EwmhFeature::ActiveWindow => self.atoms._NET_ACTIVE_WINDOW,
            EwmhFeature::Supported => self.atoms._NET_SUPPORTED,
            EwmhFeature::WmName => self.atoms._NET_WM_NAME,
            EwmhFeature::WmState => self.atoms._NET_WM_STATE,
            EwmhFeature::SupportingWmCheck => self.atoms._NET_SUPPORTING_WM_CHECK,
            EwmhFeature::WmStateFullscreen => self.atoms._NET_WM_STATE_FULLSCREEN,
            EwmhFeature::ClientList => self.atoms._NET_CLIENT_LIST,
            EwmhFeature::ClientInfo => self.atoms._NET_CLIENT_INFO,
            EwmhFeature::WmWindowType => self.atoms._NET_WM_WINDOW_TYPE,
            EwmhFeature::WmWindowTypeDialog => self.atoms._NET_WM_WINDOW_TYPE_DIALOG,
        }
    }
}

impl<C: Connection + Send + Sync + 'static> EwmhFacade for X11EwmhFacade<C> {
    fn declare_supported(&self, features: &[EwmhFeature]) -> Result<(), BackendError> {
        let atoms: Vec<u32> = features.iter().map(|f| self.feature_to_atom(*f)).collect();
        let r = self.ids.x11(self.root)?;
        self.conn.change_property32(
            PropMode::REPLACE,
            r,
            self.atoms._NET_SUPPORTED,
            AtomEnum::ATOM,
            &atoms,
        )?;
        Ok(())
    }

    fn reset_root_properties(&self) -> Result<(), BackendError> {
        for &prop in [
            self.atoms._NET_ACTIVE_WINDOW,
            self.atoms._NET_CLIENT_LIST,
            self.atoms._NET_SUPPORTED,
            self.atoms._NET_CLIENT_LIST_STACKING,
            self.atoms._NET_SUPPORTING_WM_CHECK,
        ]
        .iter()
        {
            let r = self.ids.x11(self.root)?;
            let _ = self.conn.delete_property(r, prop);
        }
        Ok(())
    }
    fn setup_supporting_wm_check(&self, wm_name: &str) -> Result<WindowId, BackendError> {
        let frame_win = self.conn.generate_id()?;
        let aux = CreateWindowAux::new()
            .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS)
            .override_redirect(1);
        let r = self.ids.x11(self.root)?;
        self.conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            frame_win,
            r,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &aux,
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            r,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            AtomEnum::WINDOW,
            &[frame_win],
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            frame_win,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            AtomEnum::WINDOW,
            &[frame_win],
        )?;
        // WM_NAME (STRING)
        x11rb::wrapper::ConnectionExt::change_property8(
            &*self.conn,
            PropMode::REPLACE,
            frame_win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            wm_name.as_bytes(),
        )?;
        Ok(self.ids.intern(frame_win))
    }

    fn set_active_window(&self, win: WindowId) -> Result<(), BackendError> {
        let w = self.ids.x11(win)?;
        let r = self.ids.x11(self.root)?;
        self.conn.change_property32(
            PropMode::REPLACE,
            r,
            self.atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            &[w],
        )?;
        Ok(())
    }

    fn clear_active_window(&self) -> Result<(), BackendError> {
        use x11rb::protocol::xproto::ConnectionExt as RawExt;
        let r = self.ids.x11(self.root)?;
        self.conn
            .delete_property(r, self.atoms._NET_ACTIVE_WINDOW)?;
        Ok(())
    }

    fn set_client_list(&self, list: &[WindowId]) -> Result<(), BackendError> {
        let r = self.ids.x11(self.root)?;
        let raw: Vec<u32> = list.iter().map(|&w| self.ids.x11(w).unwrap()).collect();
        self.conn.change_property32(
            PropMode::REPLACE,
            r,
            self.atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW,
            &raw,
        )?;
        Ok(())
    }

    fn set_client_list_stacking(&self, list: &[WindowId]) -> Result<(), BackendError> {
        let r = self.ids.x11(self.root)?;
        let raw: Vec<u32> = list.iter().map(|&w| self.ids.x11(w).unwrap()).collect();
        self.conn.change_property32(
            PropMode::REPLACE,
            r,
            self.atoms._NET_CLIENT_LIST_STACKING,
            AtomEnum::WINDOW,
            &raw,
        )?;
        Ok(())
    }
}
