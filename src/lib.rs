//! Parallel datacenter network simulator
//!
//! Throughout this crate there is a user-backend relationship between the [simulation
//! engine](synchronizer/index.html) and the model. In general, the engine should be agnostic to
//! the type of model being run, and should probably eventually be pulled out into its own crate.

use atomic_counter::RelaxedCounter;
//use crossbeam_deque::Worker;
use crate::worker::{run, Advancer};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::thread;

//use slog::*;
//use slog_async;

pub mod engine;
pub mod logger;
pub mod network;
pub mod phold;
pub mod worker;

pub struct FrozenActor<T, R>
where
    T: Ord + Copy + num::Zero,
{
    time: T,
    actor: Box<dyn Advancer<T, R> + Send>,
}

impl<T, R> Ord for FrozenActor<T, R>
where
    T: Ord + Copy + num::Zero,
{
    fn cmp(&self, other: &Self) -> Ordering {
        other.time.cmp(&self.time)
    }
}

impl<T, R> PartialOrd for FrozenActor<T, R>
where
    T: Ord + Copy + num::Zero,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(other.time.cmp(&self.time))
    }
}

impl<T, R> PartialEq for FrozenActor<T, R>
where
    T: Ord + Copy + num::Zero,
{
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl<T, R> Eq for FrozenActor<T, R> where T: Ord + Copy + num::Zero {}

/// Starts the actors on `num_cpus` workers
///
/// This function takes care of all the necessary building of the workers and connecting to launch
/// them
// TODO check if we can remove dynamic dispatch in simple cases
pub fn start<T: 'static + Ord + Copy + Debug + Send + num::Zero, R: 'static + Send + Copy>(
    num_cpus: usize,
    actors: Vec<Box<dyn Advancer<T, R> + Send>>,
) -> Vec<R> {
    // Start the workers
    let mut handles = Vec::new();
    let n_actors = actors.len();
    let shared_counter = Arc::new(RelaxedCounter::new(0));
    let task_heap = Arc::new(Mutex::new(BinaryHeap::new()));

    for actor in actors {
        let frozen = FrozenActor {
            time: T::zero(),
            actor,
        };
        task_heap.lock().unwrap().push(frozen);
    }
    for i in 0..num_cpus {
        // start this worker
        handles.push({
            let heap_clone = Arc::clone(&task_heap);
            let counter_clone = Arc::clone(&shared_counter);
            thread::spawn(move || run(i, counter_clone, n_actors, heap_clone))
        });
    }

    // Wait for the workers to be done
    let mut counts = Vec::new();
    for h in handles {
        let local_counts: Vec<R> = h.join().unwrap();
        counts.extend(local_counts);
    }

    counts
}
