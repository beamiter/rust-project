use ndarray::prelude::*;

pub struct Domain {
    pub close: Array1<f64>,
    pub far: Array1<f64>,
}

impl Domain {
    pub fn new(close: Array1<f64>, far: Array1<f64>) -> Domain {
        Domain { close, far }
    }
}
