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

use atomic_counter::{AtomicCounter, RelaxedCounter};
//use crossbeam_deque::{Steal, Stealer, Worker};
//use crossbeam_queue::spsc::{Consumer, Producer};
//use crossbeam_utils::Backoff;
use crate::FrozenActor;
use std::collections::BinaryHeap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum ActorState<T, R>
where
    T: Ord + Copy + num::Zero,
{
    Continue(T),
    Done(R),
}

/// Advancer trait to be implemented by the simulation actors
///
/// The advancer trait allows the workers to pick up the actors and keep them going
pub trait Advancer<T, R>
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
///
/// TODO: pulled from crossbeam's documentation, figure more about how it works
pub fn run<T: Ord + Copy + Debug + num::Zero, R: Send>(
    id: usize,
    counter: Arc<RelaxedCounter>,
    n_tasks: usize,
    task_heap: Arc<Mutex<BinaryHeap<FrozenActor<T, R>>>>,
) -> Vec<R> {
    println!("{} start", id);
    let mut counts = Vec::new();

    // initial task
    let mut task = task_heap.lock().unwrap().pop();
    loop {
        if let Some(mut frozen_actor) = task {
            //println!("{} task start", id);
            match frozen_actor.actor.advance() {
                ActorState::Continue(time) => {
                    frozen_actor.time = time;
                    let mut heap = task_heap.lock().unwrap();
                    heap.push(frozen_actor);
                    task = heap.pop();
                }
                ActorState::Done(count) => {
                    counts.push(count);
                    counter.inc();
                    task = task_heap.lock().unwrap().pop();
                }
            }
        //println!("{} task done", id);
        } else if counter.get() == n_tasks {
            println!("{} finished", id);
            return counts;
        } else {
            task = task_heap.lock().unwrap().pop();
        }
    }
}

/*
#[cfg(test)]
mod test {
    use crate::worker::{run, Advancer};
    use crossbeam_deque::{Injector, Worker};
    use std::sync::Arc;

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

    impl Advancer for DummyAdvance {
        fn advance(&mut self) -> bool {
            self.count += 1;
            println!("{}: {}", self.id, self.count);

            // Done
            return self.count < self.limit;
        }
    }

    #[test]
    fn test_advance() {
        let dummy = &mut DummyAdvance::new(0, 3);
        assert!(dummy.advance());
        assert!(dummy.advance());
        assert!(!dummy.advance()); // stops on the 3rd
    }

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
}
*/
