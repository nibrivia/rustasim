//! This module takes care of scheduling the actors appropriately
//!
//! My current guess is that we want the actors scheduled in round-robin fashing: keep running
//! until you can't go any further (stall), and then wait until someone stalls on you. That's ideal
//! in that it minimizes switching. This is also good because it ideally mimizes null-message
//! passing.
//!
//! To implement this without actually monitoring everything, I propose running a certain number of
//! workers, each advancing a particular actor until it stalls, then putting that actor to the back
//! of the queue. Ideally this actor will next be scheduled when all of its neighbours will have
//! made progress.
//!
//! To actually do this, each actor needs an "advance" method that will return when it can't make
//! any more progress, and can be called repeatedly. This module can take these "advanceables"
//! (trait?) and schedule them via crossbeam's work-stealing queue (insert link).

use crate::FrozenActor;
use atomic_counter::{AtomicCounter, RelaxedCounter};
use parking_lot::Mutex;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;

/// Convenience wrapper for a reference counted, distributed heap of frozen actors...
pub type LockedTaskHeap<T, R> = Arc<Mutex<VecDeque<FrozenActor<T, R>>>>;

/// Return value for actors to use to signal their state to the workers
#[derive(Debug)]
pub enum ActorState<T, R>
where
    T: Ord + Copy + num::Zero,
{
    /// The simulation was able to advance up to this time
    Continue(T),

    /// The simulation is done, returning inner type R
    Done(R),
}

/// Advancer trait to be implemented by the simulation actors
///
/// The advancer trait allows the workers to pick up the actors and keep them going
pub trait Advancer<T, R>: Debug
where
    T: Ord + Copy + num::Zero,
{
    /// Advances this actor until it can no longer.
    ///
    /// The return value indicates whether it should get rescheduled or no. `true` reschedules,
    /// `false` assumes it is done.
    fn advance(&mut self) -> ActorState<T, R>;
}

/// Runs until no more progress can be made at all...
pub fn run<T: Ord + Copy + Debug + num::Zero, R: Send>(
    _id: usize,
    counter: Arc<RelaxedCounter>,
    n_tasks: usize,
    task_heap: Vec<LockedTaskHeap<T, R>>,
) -> Vec<R> {
    let mut counts = Vec::new();

    // rng
    let mut rng = thread_rng();

    // initial task
    let mut task = task_heap.choose(&mut rng).unwrap().lock().pop_front();
    loop {
        if let Some(mut frozen_actor) = task {
            match frozen_actor.actor.advance() {
                ActorState::Continue(time) => {
                    frozen_actor.time = time;
                    let mut heap = task_heap.choose(&mut rng).unwrap().lock();
                    heap.push_back(frozen_actor);
                    task = heap.pop_front();
                }
                ActorState::Done(count) => {
                    counts.push(count);
                    counter.inc();
                    task = task_heap.choose(&mut rng).unwrap().lock().pop_front();
                }
            }
        } else if counter.get() == n_tasks {
            return counts;
        } else {
            //println!("huh");
            task = task_heap.choose(&mut rng).unwrap().lock().pop_front();
        }
    }
}

#[cfg(test)]
mod test {
    use crate::worker::*;

    #[derive(Debug)]
    struct DummyAdvance {
        id: usize,
        count: u64,
        limit: u64,
    }

    impl DummyAdvance {
        fn new(id: usize, limit: u64) -> DummyAdvance {
            DummyAdvance {
                id,
                count: 0,
                limit,
            }
        }

        fn _count(&self) -> u64 {
            self.count
        }
    }

    impl Advancer<u64, ()> for DummyAdvance {
        fn advance(&mut self) -> ActorState<u64, ()> {
            self.count += 1;
            println!("{}: {}", self.id, self.count);

            // Done
            if self.count < self.limit {
                ActorState::Continue(self.count)
            } else {
                ActorState::Done(())
            }
        }
    }

    #[test]
    fn test_advance() {
        let dummy = &mut DummyAdvance::new(0, 3);
        if let ActorState::Continue(_) = dummy.advance() {
        } else {
            assert!(false);
        }

        if let ActorState::Continue(_) = dummy.advance() {
        } else {
            assert!(false);
        }

        if let ActorState::Done(_) = dummy.advance() {
        } else {
            assert!(false);
        }
    }

    /*
        #[test]
        fn test_single_thread() {
            let local: Worker<Box<dyn Advancer + Send>> = Worker::new_fifo();
            let global = Injector::new();
            let stealers = Vec::new();

            let advancer = Box::new(DummyAdvance::new(1, 3));
            local.push(advancer);

            let advancer = Box::new(DummyAdvance::new(2, 5));
            local.push(advancer);

            //let thread_worker = ThreadWorker::new(local, global, stealers);
            run(&local, Arc::new(global), &stealers);

            // TODO find auto testing
            assert!(true);
        }
    */
}
