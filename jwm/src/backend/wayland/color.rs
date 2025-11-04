use crate::backend::api::ColorAllocator;

pub struct WaylandColorAllocator {}

impl WaylandColorAllocator {
    pub fn new() -> Self {
        Self {}
    }
}

impl ColorAllocator for WaylandColorAllocator {

}
