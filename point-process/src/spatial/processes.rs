use super::domains::Domain;

use rand::prelude::*;
use rand::rngs::SmallRng;
use rand_distr::{Distribution, Poisson};

use ndarray::{stack, Array, Array1, Array2, ArrayView2, Axis};

static XORESHHIFT_ERR: &str = "Unable to create XorShift rng from thread local rng";

pub fn poisson_process(lambda: f64, domain: &Domain) -> Array2<f64> {
    let ref mut rng = thread_rng();
    let far = &domain.far;
    let close = &domain.close;

    let d = far.shape()[0];
    let area = (0..d).fold(1.0, |area, i| area * (far[i] - close[i]));

    let fish = Poisson::new(lambda * area).unwrap();
    let num_events: u64 = fish.sample(rng) as u64;

    let mut srng = SmallRng::from_rng(rng).expect(XORESHHIFT_ERR);

    let events: Vec<Array2<f64>> = (0..num_events)
        .map(|_| {
            let mut ev: Array1<f64> = Array::zeros((d,));

            for i in 0..d {
                ev[i] = srng.gen_range(close[i]..far[i]);
            }

            ev.to_shape((1, d)).unwrap()
        })
        .collect();

    let events_ref: Vec<ArrayView2<f64>> = events.iter().map(|ev| ev.view()).collect();

    stack(Axis(0), events_ref.as_slice()).unwrap()
}
