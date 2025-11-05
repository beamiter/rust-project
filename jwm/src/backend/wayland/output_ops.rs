use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};

pub struct WaylandOutputOps {}

impl WaylandOutputOps {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl OutputOps for WaylandOutputOps {
    fn screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: 960,
            height: 600,
        }
    }

    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        vec![]
    }
}
