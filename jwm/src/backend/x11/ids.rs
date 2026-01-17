// src/backend/x11/ids.rs
use crate::backend::common_define::WindowId;
use crate::backend::error::BackendError;
use std::collections::HashMap;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default)]
pub struct X11IdRegistry {
    next: Arc<AtomicU64>,
    x11_to_wid: Arc<RwLock<HashMap<u32, WindowId>>>,
    wid_to_x11: Arc<RwLock<HashMap<WindowId, u32>>>,
}

impl X11IdRegistry {
    pub fn new(start: u64) -> Self {
        Self {
            next: Arc::new(AtomicU64::new(start)),
            x11_to_wid: Arc::new(RwLock::new(HashMap::new())),
            wid_to_x11: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 将 X11 window(u32) intern 成 WindowId（稳定）
    pub fn intern(&self, x11: u32) -> WindowId {
        if let Some(id) = self.x11_to_wid.read().unwrap().get(&x11).copied() {
            return id;
        }
        // 写锁双检
        let mut w = self.x11_to_wid.write().unwrap();
        if let Some(id) = w.get(&x11).copied() {
            return id;
        }

        let id = WindowId::from_raw(self.next.fetch_add(1, Ordering::Relaxed));
        w.insert(x11, id);
        self.wid_to_x11.write().unwrap().insert(id, x11);
        id
    }

    pub fn x11(&self, id: WindowId) -> Result<u32, BackendError> {
        self.wid_to_x11
            .read()
            .unwrap()
            .get(&id)
            .copied()
            .ok_or(BackendError::NotFound("WindowId not mapped to X11 window"))
    }

    pub fn remove_x11(&self, x11: u32) {
        if let Some(id) = self.x11_to_wid.write().unwrap().remove(&x11) {
            self.wid_to_x11.write().unwrap().remove(&id);
        }
    }
}
