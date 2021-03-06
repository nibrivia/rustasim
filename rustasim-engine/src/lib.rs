#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

//! Parallel datacenter network simulator
//!
//! Throughout this crate there is a user-backend relationship between the [simulation
//! engine](engine/index.html) and the model. In general, the engine should be agnostic to
//! the type of model being run, and should probably eventually be pulled out into its own crate.

use atomic_counter::RelaxedCounter;
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;
use std::thread;

//use slog::*;
//use slog_async;

mod engine;
mod err;
pub mod spsc;
mod tree;
mod worker;

pub use self::engine::{Event, EventType, Merger};
pub use self::err::{PopError, PushError};
pub use self::worker::{run, ActorState, Advancer, LockedTaskHeap};

/// Maintains the state of the actor while it's at rest
#[derive(Debug)]
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
    mut actors: Vec<Box<dyn Advancer<T, R> + Send>>,
) -> Vec<R> {
    // Start the workers
    let n_actors = actors.len();
    let shared_counter = Arc::new(RelaxedCounter::new(0));

    // Initialize the heaps
    let n_heaps = std::cmp::min(16, n_actors);
    let mut heaps = Vec::new();
    for _ in 0..n_heaps {
        let task_heap: LockedTaskHeap<T, R> = Arc::new(Mutex::new(VecDeque::new()));
        heaps.push(task_heap);
    }

    for (i, actor) in actors.drain(..).enumerate() {
        let heap_ix = i % n_heaps;
        let frozen = FrozenActor {
            time: T::zero(),
            actor,
        };
        heaps[heap_ix].lock().push_back(frozen);
    }

    let mut handles = Vec::new();
    for i in 0..num_cpus {
        // start this worker
        handles.push({
            let cloned_heaps = heaps.iter().map(|x| Arc::clone(&x)).collect();
            let counter_clone = Arc::clone(&shared_counter);
            thread::spawn(move || run(i, counter_clone, n_actors, cloned_heaps))
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
