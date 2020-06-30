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

use crossbeam_deque::{Injector, Steal, Stealer, Worker};


trait Advancer {
    fn advance(&mut self);
}

#[derive(Debug)]
struct ThreadWorker<T> {
    local: Worker<T>,
    global: Injector<T>,
    stealers: Vec<Stealer<T>>,
}

impl<T: Advancer> ThreadWorker<T> {
    pub fn run(self) {
        loop {
            let mut task: T = self.local.pop().or_else(|| {
                // Otherwise, we need to look for a task elsewhere.
                std::iter::repeat_with(|| {
                    // Try stealing a batch of tasks from the global queue.
                    self.global.steal_batch_and_pop(&self.local)
                        // Or try stealing a task from one of the other threads.
                        .or_else(|| self.stealers.iter().map(|s| s.steal()).collect())
                })
                // Loop while no task was stolen and any steal operation needs to be retried.
                .find(|s| !s.is_retry())
                // Extract the stolen task, if there is one.
                .and_then(|s| s.success())
            }).unwrap(); // TODO investigate

            task.advance();
            self.local.push(task);
        }
    }
}
