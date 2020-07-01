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

use crossbeam_deque::{Steal, Stealer, Worker};
use crossbeam_queue::spsc::{Consumer, Producer};
//use std::sync::Arc;

pub enum ActorState {
    Continue,
    Done(u64),
}

/// Advancer trait to be implemented by the simulation actors
///
/// The advancer trait allows the workers to pick up the actors and keep them going
pub trait Advancer {
    /// Advances this actor until it can no longer.
    ///
    /// The return value indicates whether it should get rescheduled or no. `true` reschedules,
    /// `false` assumes it is done.
    fn advance(&mut self) -> ActorState;
}

/// Runs until no more progress can be made at all...
///
/// TODO: pulled from crossbeam's documentation, figure more about how it works
pub fn run(
    id: usize,
    local: &Worker<Box<dyn Advancer + Send>>,
    prev: Consumer<Box<dyn Advancer + Send>>,
    next: Producer<Box<dyn Advancer + Send>>,
    //global: Arc<Injector<Box<dyn Advancer + Send>>>,
    stealers: Vec<Stealer<Box<dyn Advancer + Send>>>,
) -> Vec<u64> {
    println!("{} start", id);
    let mut counts = Vec::new();
    loop {
        while let Ok(task) = prev.pop() {
            local.push(task);
        }

        let mut task: Option<Box<dyn Advancer + Send>> = local.pop();

        // Try first to steal from our neighbours, in reverse order
        if let None = task {
            for _ in 0..2 {
                if let Some(_) = task {
                    break;
                }

                for s in &stealers {
                    let mut r = s.steal();
                    while r.is_retry() {
                        //println!("{} retry", id);
                        r = s.steal();
                    }

                    match r {
                        Steal::Empty => continue,
                        Steal::Success(t) => {
                            task = Some(t);
                            break;
                        }
                        Steal::Retry => unreachable!(),
                    }
                }
            }
        }

        /*local.pop().or_else(|| {
            // Otherwise, we need to look for a task elsewhere.
            std::iter::repeat_with(|| {
                // Try stealing a batch of tasks from the global queue.
                global
                    .steal_batch_and_pop(local)
                    // Or try stealing a task from one of the other threads.
                    .or_else(|| stealers.iter().map(|s| s.steal()).collect())
            })
            // Loop while no task was stolen and any steal operation needs to be retried.
            .find(|s| !s.is_retry())
            // Extract the stolen task, if there is one.
            .and_then(|s| s.success())
        });*/

        if let Some(mut actor) = task {
            //println!("{} task start", id);
            match actor.advance() {
                ActorState::Continue => next.push(actor).unwrap(),
                ActorState::Done(count) => counts.push(count),
            }
        //println!("{} task done", id);
        } else {
            println!("{} finished", id);
            return counts;
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
