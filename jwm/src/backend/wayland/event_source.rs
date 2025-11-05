use crate::backend::api::{BackendEvent, EventSource};

pub struct WaylandEventSource {}

impl WaylandEventSource {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl EventSource for WaylandEventSource {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
        Ok(None)
    }
}
