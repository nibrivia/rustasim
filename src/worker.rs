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

use crossbeam_deque::{Injector, Stealer, Worker};

trait Advancer {
    fn advance(&mut self) -> bool;
}

#[derive(Debug)]
struct ThreadWorker<T> {
    local: Worker<T>,
    global: Injector<T>,
    stealers: Vec<Stealer<T>>,
}

impl<T: Advancer> ThreadWorker<T> {
    /// Runs until no more progress can be made at all...
    ///
    /// TODO: pulled from crossbeam's documentation, figure more about how it works
    pub fn run(self) {
        loop {
            let task = self.local.pop().or_else(|| {
                // Otherwise, we need to look for a task elsewhere.
                std::iter::repeat_with(|| {
                    // Try stealing a batch of tasks from the global queue.
                    self.global
                        .steal_batch_and_pop(&self.local)
                        // Or try stealing a task from one of the other threads.
                        .or_else(|| self.stealers.iter().map(|s| s.steal()).collect())
                })
                // Loop while no task was stolen and any steal operation needs to be retried.
                .find(|s| !s.is_retry())
                // Extract the stolen task, if there is one.
                .and_then(|s| s.success())
            });

            if let Some(mut actor) = task {
                if actor.advance() {
                    self.local.push(actor);
                }
            } else {
                // nothing to do, return?
                return;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::worker::{Advancer, ThreadWorker};
    use crossbeam_deque::{Injector, Worker};

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
        let local = Worker::<DummyAdvance>::new_fifo();
        let global = Injector::<DummyAdvance>::new();
        let stealers = Vec::new();

        let advancer = DummyAdvance::new(1, 3);
        local.push(advancer);

        let advancer = DummyAdvance::new(2, 5);
        local.push(advancer);

        let thread_worker = ThreadWorker {
            local,
            global,
            stealers,
        };
        thread_worker.run();

        // TODO find auto testing
        assert!(true);
    }
}
