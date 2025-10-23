// src/backend/wayland/event_source.rs
use crate::backend::api::{BackendEvent, EventSource};
use crossbeam_channel::{Receiver, TryRecvError};

pub struct WlEventSource {
    rx: Receiver<BackendEvent>,
}

impl WlEventSource {
    pub fn new(rx: Receiver<BackendEvent>) -> Self {
        Self { rx }
    }
}

impl EventSource for WlEventSource {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
        match self.rx.try_recv() {
            Ok(ev) => Ok(Some(ev)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Ok(None),
        }
    }
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
