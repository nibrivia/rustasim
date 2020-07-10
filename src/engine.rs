//! Drives the simulation
//!
//! This module implements (almost) all that is needed for a conservative event-driven simulation.
//!
//! The main ideas behind a conservative scheduling approach is to return all events to the actor
//! in a monotonically increasing fashion. In order to do this without risking later on returning
//! an event that would break causality, we need to wait to hear from all our neighbours. This
//! sometimes causes a deadlock when two (or more) neighbours are waiting on each other.
//!
//! In order to resolve the deadlock, one can send "null-messages". Essentially telling neighbours
//! that we have nothing to do. Fundamentally this is an actor-level action: there is no way the
//! simulation engine knows enough about the simulation model to know which neighbours have to hear
//! from us, or how long in the future it is safe for them to advance. Therefore the engine returns
//! a "Stalled" event type at which point the actor *must* update all relevant neighbours in order
//! for the simulation to progress.
//!
// TODO description of when the null-message should be sent and what it should look like

use crossbeam_queue::spsc;
use std::cmp::Ordering;
use std::mem;

// TODO update description to match the parametrized Events we have
/// Event types and their associated data.
///
/// These are all simulation-driving related events. None of them have anything to do with the
/// specific model being simulated. Each model will presumably require different event models, to
/// be passed in (probably as an enum, but as you wish) as `<U>`.
///
/// There are two necessary event types: `Stalled`, and `Null`. Both need processing by the
/// associated actor, and are required for all simulations.
///
/// The `Close` event type is sufficiently universal that it will presumably also stay here.
#[derive(Debug)]
pub enum EventType<U> {
    /// Define `<U>` as you wish for your model
    ModelEvent(U),

    /// The simulation is stalled, the actor must update its neighbours with null-events
    Stalled,

    /// It is safe for the simulaiton to advance to this time.
    ///
    /// Actors may also assert `unreachable!` for this event type. It is processed internally and
    /// never sent to the actor.
    Null,

    /// Simulation end.
    ///
    /// Actors should update neighbours appropriately to ensure they will terminate.
    Close,
}

/// Fully describes an event.
///
/// The `src` field will almost certainly get modified as it works its way through the event
/// merger. Event creators should set `src` to be their own, unique, id, the
/// [`Merger`](struct.Merger.html) of the receiving actor will appropriately translate it into and
/// index.
///
/// Receivers should assume `src` is the *index* of the source, and not the id.
///
/// Events are ordered by their time.
#[derive(Debug)]
pub struct Event<U> {
    pub time: u64,
    //pub real_time: u128,
    pub src: usize,
    pub event_type: EventType<U>,
}

impl<U> Ord for Event<U> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.time.cmp(&self.time)
    }
}

impl<U> PartialOrd for Event<U> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<U> PartialEq for Event<U> {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl<U> Eq for Event<U> {}

/// Manages the input queues and returns the next [`Event`](struct.Event.html) to be processed.
///
/// The events returned by `Merger` are monotonically increasing and come from either neighbours,
/// or from the Merger itself upon a potential Stall.
///
/// # Examples
///
/// ```
/// // empty model for testing
/// // TODO
///
/// ```
#[derive(Debug)]
pub struct Merger<U> {
    id: usize,
    //start: Instant,
    // the input queues
    in_queues: Vec<spsc::Consumer<Event<U>>>,
    n_layers: usize,

    paths: Vec<usize>,

    // the queue to pull from
    winner_q: usize,
    safe_time: u64,

    // the loser queue
    loser_e: Vec<Event<U>>,

    // logger
    //log: slog::Logger,
    ix_to_id: Vec<usize>,
}

/// Returns indices into an 1-indexed array a tree with `n_nodes` leaves from left-to-right.
///
/// This means that it takes the leftmost leaf first, goes up to its parent, then the second
/// leftmost leaf, and so on...
///
/// If the tree has 6 leaves, it will have 2 leaves at 4, 5, and 3:
/// ```no-run
///     1
///  2     3
/// 4 5    |
/// ^ ^    ^
/// ```
///
/// And the left-to-right walk will be `[4, 2, 5, 1, 3]`.
///
/// # Examples
///
/// ```
/// //let indices = rustasim::synchronizer::ltr_walk(6);
/// //assert_eq!(indices, vec![4, 2, 5, 1, 3]);
/// ```
///
fn ltr_walk(n_nodes: usize) -> Vec<usize> {
    let n_layers = (n_nodes as f32).log2().ceil() as usize;

    // visited structure
    let mut visited: Vec<bool> = Vec::new();
    for _ in 0..n_nodes + 1 {
        visited.push(false);
    }

    let mut cur_index = 2_usize.pow((n_layers - 1) as u32);

    let mut indices = Vec::new();

    indices.push(cur_index);
    visited[cur_index] = true;
    let mut going_up = true;

    for _ in 0..n_nodes - 1 {
        if going_up {
            while visited[cur_index] {
                cur_index /= 2;
            }
        } else {
            if cur_index * 2 + 1 >= n_nodes {
                going_up = true;
                continue;
            }

            cur_index = cur_index * 2 + 1;
            while cur_index * 2 < n_nodes {
                cur_index *= 2;
            }
        }

        indices.push(cur_index);
        visited[cur_index] = true;

        going_up = !going_up;
    }

    // hacky but w/e
    if visited[0] {
        indices.pop();
    }

    indices
}

impl<U> Merger<U>
where
    U: std::fmt::Debug,
{
    /// Builds a new merger from a set of input queues
    pub fn new(
        in_queues: Vec<spsc::Consumer<Event<U>>>,
        id: usize,
        ix_to_id: Vec<usize>,
        //log: slog::Logger,
        //start: Instant,
    ) -> Merger<U> {
        let mut loser_e = Vec::new();
        let winner_q = 0;

        // index 0 never gets used
        loser_e.push(Event {
            time: 0,
            //real_time: 0,
            event_type: EventType::Null,
            src: 0,
        });

        // TODO hacky
        // fill with placeholders
        for i in 1..in_queues.len() {
            loser_e.push(Event {
                time: 0,
                //real_time: 0,
                event_type: EventType::Null,
                src: i,
            });
        }

        // appropriately set the source
        for (loser, ix) in ltr_walk(in_queues.len()).iter().enumerate() {
            loser_e[*ix] = Event {
                time: 0,
                //real_time: 0,
                event_type: EventType::Null,
                src: loser + 1,
            };
        }

        // helfpul number
        let n_queues = in_queues.len();
        let n_layers = (n_queues as f32).log2().ceil() as usize;
        let largest_full_layer = 2_usize.pow((n_queues as f32).log2().floor() as u32);
        let last_layer_max_i = ((n_queues + largest_full_layer - 1) % largest_full_layer + 1) * 2;
        let offset = (last_layer_max_i + 1) / 2;

        let mut paths = Vec::new();
        for ix in 0..n_queues {
            let v_ix = if ix > last_layer_max_i {
                (ix - offset) * 2
            } else {
                ix
            };

            let mut index = 0;

            assert!(n_layers > 0);
            for level in (0..n_layers).rev() {
                // compute index
                // TODO pre-compute?
                let base_offset = 2_usize.pow(level as u32);
                index = base_offset + v_ix / 2_usize.pow((n_layers - level) as u32);

                // skip if we're out of bounds
                if index >= loser_e.len() {
                    continue;
                }
                break;
            }

            paths.push(index);
        }

        Merger {
            id,

            in_queues,
            n_layers,

            winner_q,
            safe_time: 0,

            paths,

            loser_e,

            //log,
            ix_to_id,
        }
    }

    /// Non-blocking next event. Used for testing.
    fn _try_pop(&mut self) -> Option<Event<U>> {
        if !self.in_queues[self.winner_q].is_empty() {
            self.next()
        } else {
            None
        }
    }
}

impl<U> Iterator for Merger<U>
where
    U: std::fmt::Debug,
{
    type Item = Event<U>;

    // blocks until it has something to return
    fn next(&mut self) -> Option<Self::Item> {
        // The state of this must be mostly done except for the previous winner
        loop {
            // get the path up
            let mut index = self.paths[self.winner_q];
            let q = &self.in_queues[self.winner_q]; // avoids regularly indexing into that vec

            // TODO handle safe_time
            // TODO handle when more than one path is empty?

            // get the new candidate
            let mut new_winner_e = match q.pop() {
                Err(_) => {
                    Event {
                        time: self.safe_time,
                        //real_time: self.start.elapsed().as_nanos(),
                        src: self.winner_q,
                        event_type: EventType::Stalled,
                    }
                }
                Ok(event) => event,
            };

            // change the source id->ix now
            new_winner_e.src = self.winner_q;

            // go up our path, noting the loser as we go
            while index != 0 {
                // get current loser
                let cur_loser_t = self.loser_e[index].time;

                // The current loser wins, swap with our candidate, move up
                if cur_loser_t < new_winner_e.time {
                    mem::swap(&mut new_winner_e, &mut self.loser_e[index]);
                } else if cur_loser_t == new_winner_e.time {
                    // if there's a tie, the Stalled event looses
                    if let EventType::Stalled = new_winner_e.event_type {
                        //if let EventType::Stalled = self.loser_e[index].event_type {
                        //} else {
                        mem::swap(&mut new_winner_e, &mut self.loser_e[index]);
                        //}
                    }
                }

                index /= 2;
            }

            // We need this to know what path to go up next time...
            self.winner_q = new_winner_e.src;

            // We need this to return events even if we don't have new events coming in...
            self.safe_time = new_winner_e.time;

            /*trace!(
                self.log,
                "{},{},{},{:?}",
                //new_winner_e.real_time,
                new_winner_e.time,;
                self.id,
                self.ix_to_id[new_winner_e.src],
                new_winner_e.event_type,
            );*/

            // Null events are only useful for us
            if let EventType::Null = new_winner_e.event_type {
                continue;
            }

            // If we were gonna stall but we can make progress, don't
            if let EventType::Stalled = new_winner_e.event_type {
                if self.in_queues[new_winner_e.src].len() > 0 {
                    continue;
                }
            }

            return Some(new_winner_e);
        }
    }
}

// TODO make test-able again
#[cfg(test)]
mod test_merger {
    use crate::engine::*;
    use crossbeam_queue::spsc;
    use std::{thread, time};

    #[derive(Debug)]
    enum EmptyModel {
        None,
    }

    #[test]
    fn test_ltr() {
        let ix = ltr_walk(2);
        let expected = vec![1];
        assert_eq!(ix, expected);

        let ix = ltr_walk(3);
        let expected = vec![2, 1];
        assert_eq!(ix, expected);

        let ix = ltr_walk(4);
        let expected = vec![2, 1, 3];
        assert_eq!(ix, expected);

        let ix = ltr_walk(5);
        let expected = vec![4, 2, 1, 3];
        assert_eq!(ix, expected);

        let ix = ltr_walk(13);
        let expected = vec![8, 4, 9, 2, 10, 5, 11, 1, 12, 6, 3, 7];
        assert_eq!(ix, expected);
    }

    #[test]
    fn test_merge_2() {
        test_interleave(2, 5);
        test_pushpop(2, 5);
        test_ties(2, 5);
    }

    #[test]
    fn test_merge_4() {
        test_interleave(4, 10);
        test_pushpop(4, 10);
        test_ties(4, 10);
    }

    #[test]
    fn test_merge_5() {
        test_interleave(5, 10);
        test_pushpop(5, 10);
        test_ties(5, 10);
    }

    #[test]
    fn test_merge_7() {
        test_interleave(7, 10);
        test_pushpop(7, 10);
        test_ties(7, 10);
    }

    #[test]
    fn test_merge_many_interleave() {
        for n_queues in 3..20 {
            println!("{} queues =======================", n_queues);
            test_interleave(n_queues, n_queues + 5);
        }
    }

    #[test]
    fn test_merge_many_threads() {
        for n_queues in 3..20 {
            println!("{} queues =======================", n_queues);
            test_pushpop(n_queues, n_queues + 5);
        }
    }

    #[test]
    fn test_merge_many_ties() {
        for n_queues in 3..20 {
            println!("{} queues =======================", n_queues);
            test_ties(n_queues, n_queues + 5);
        }
    }

    fn test_interleave(n_queues: usize, n_events: usize) {
        println!("Interleaving");
        // Create our event queues
        let mut prod_qs = Vec::new();
        let mut cons_qs = Vec::new();

        for _ in 0..n_queues {
            let (prod, cons) = spsc::new(128);
            prod_qs.push(prod);
            cons_qs.push(cons);
        }

        // Merger
        let mut merger = Merger::<EmptyModel>::new(cons_qs, 0, vec![]);

        //prod_qs
        println!("Pushing events");
        for (src, prod) in prod_qs.iter().enumerate() {
            for i in 1..n_events + 1 {
                let e = Event {
                    time: (src * 1 + i) as u64,
                    src,
                    event_type: EventType::ModelEvent(EmptyModel::None),
                };
                println!("  {} <- {:?}", i, e);
                prod.push(e).unwrap();
            }

            // close sentinel
            let e = Event {
                time: 100000,
                src,
                event_type: EventType::Close,
            };
            prod.push(e).unwrap();
        }

        println!("\nPopping events");
        let mut event_count = 0;
        let mut cur_time = 0;

        while let Some(event) = merger._try_pop() {
            println!("    => {:?}", event);

            assert!(
                cur_time <= event.time,
                "Time invariant violated. Previous event was @{}, current event @{}",
                cur_time,
                event.time
            );
            cur_time = event.time;
            event_count += 1;
        }

        // n_q*n_e events, 1 close event
        let expected_count = n_queues * n_events + 1;

        assert_eq!(
            event_count, expected_count,
            "Expected {} events, saw {}",
            expected_count, event_count
        );
    }

    fn test_pushpop(n_queues: usize, n_events: usize) {
        // Create our event queues
        let mut prod_qs = Vec::new();
        let mut cons_qs = Vec::new();

        for _ in 0..n_queues {
            let (prod, cons) = spsc::new(128);
            prod_qs.push(prod);
            cons_qs.push(cons);
        }

        // Merger
        let mut merger = Merger::<EmptyModel>::new(cons_qs, 0, vec![]);

        // checker vars

        println!("\nMultithreaded pushing and popping events");
        let handle = thread::spawn(move || {
            // n_q*n_e events, 1 close event
            let expected_count = n_queues * n_events + 1;

            let mut event_count = 0;
            let mut cur_time = 0;

            // pop as much as we can
            while event_count < expected_count {
                let event = merger.next().unwrap();
                //println!("    => {:?}", event);
                if let EventType::Stalled = event.event_type {
                    continue;
                }

                assert!(
                    cur_time <= event.time,
                    "Time invariant violated. Previous event was @{}, current event @{}",
                    cur_time,
                    event.time
                );
                cur_time = event.time;
                event_count += 1;
            }

            if let Some(event) = merger._try_pop() {
                assert!(
                    false,
                    "Merger should not have any more events, got {:?}",
                    event
                );
            }

            assert_eq!(
                event_count, expected_count,
                "Expected {} events, saw {}",
                expected_count, event_count
            );
        });

        for (src, prod) in prod_qs.iter().enumerate().rev() {
            for i in 1..n_events + 1 {
                let e = Event {
                    time: (src * 1 + 4 * i) as u64,
                    src,
                    event_type: EventType::ModelEvent(EmptyModel::None),
                };
                println!("  {} <- {:?}", i, e);
                prod.push(e).unwrap();
                thread::sleep(time::Duration::from_micros(1));
            }

            // close sentinel
            let e = Event {
                time: 100000,
                src,
                event_type: EventType::Close,
            };
            prod.push(e).unwrap();
        }

        handle.join().unwrap();
    }

    fn test_ties(n_queues: usize, n_events: usize) {
        println!("\nTie checking");
        // Create our event queues
        let mut prod_qs = Vec::new();
        let mut cons_qs = Vec::new();

        for _ in 0..n_queues {
            let (prod, cons) = spsc::new(128);
            prod_qs.push(prod);
            cons_qs.push(cons);
        }

        // Merger
        let mut merger = Merger::<EmptyModel>::new(cons_qs, 0, vec![]);

        let mut cur_time = 0;

        let mut success = true;

        for time in 0..n_events {
            println!("\nPushing events @{}", time);
            for (ix, prod) in prod_qs.iter().enumerate() {
                let e = Event {
                    time: time as u64,
                    src: ix,
                    event_type: EventType::ModelEvent(EmptyModel::None),
                };
                println!("  {} <- {:?}", ix, e);
                prod.push(e).unwrap();
            }

            println!("Popping events");

            let mut event_count = 0;
            while let Some(event) = merger.next() {
                // break if we're stalled
                if let EventType::Stalled = event.event_type {
                    // current time -> we're done
                    if event.time == time as u64 {
                        break;
                    }

                    // past time
                    //continue;
                }

                println!("    => {:?}", event);

                // check time invariant
                assert!(
                    cur_time <= event.time,
                    "Time invariant violated. Previous event was @{}, current event @{}",
                    cur_time,
                    event.time
                );

                // update state
                cur_time = event.time;
                if let EventType::Stalled = event.event_type {
                } else {
                    event_count += 1;
                }
            }

            // check all our messages got out
            success = success && event_count == n_queues;

            println!(
                //n_queues, event_count,
                "Expected {} events, saw {}",
                n_queues,
                event_count
            );
        }

        assert!(success);
    }
}
