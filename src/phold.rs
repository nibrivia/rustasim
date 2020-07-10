//! Datacenter network model

use crossbeam_queue::spsc;
use crossbeam_queue::spsc::{Consumer, Producer};
use rand;
use rand::distributions::{Distribution, Uniform};
use std::time::Instant;

use crate::engine::*;
use crate::worker::{ActorState, Advancer};

const Q_SIZE: usize = 64;
const T_MULT: u64 = 8;
const LOOKAHEAD: u64 = 1 * T_MULT;

/// Generic event
type PHOLDEvent = ();

pub type FullEvent = crate::engine::Event<PHOLDEvent>;

/// PHOLD actor
struct Actor {
    pub id: usize,
    time_limit: u64,
    unif: Uniform<usize>,

    merger: Merger<PHOLDEvent>,
    out_queues: Vec<Producer<FullEvent>>,
    out_times: Vec<u64>,

    //_ix_to_id: Vec<usize>,
    pub count: u64,
}

impl Actor {
    fn new(
        id: usize,
        out_queues: Vec<Producer<FullEvent>>,
        in_queues: Vec<Consumer<FullEvent>>,
        time_limit: u64,
    ) -> Actor {
        let mut _ix_to_id = Vec::new();
        let mut out_times = Vec::new();
        for ix in 0..out_queues.len() {
            _ix_to_id.push(ix);
            out_times.push(0);

            if ix == id {
                // send ourselves an initial event
                //for _ in 0..out_queues.len() {
                for _ in 0..out_queues.len() {
                    out_queues[ix]
                        .push(Event {
                            event_type: EventType::ModelEvent(()),
                            src: id,
                            time: LOOKAHEAD,
                        })
                        .unwrap();
                }
            } else {
                // initialize everyone else
                out_queues[ix]
                    .push(Event {
                        event_type: EventType::Null,
                        src: id,
                        time: LOOKAHEAD,
                    })
                    .unwrap();
            }
        }

        Actor {
            id,
            time_limit,
            unif: Uniform::from(0..out_queues.len()),

            merger: Merger::new(in_queues, id, _ix_to_id),
            out_queues,
            out_times,

            //_ix_to_id,
            count: 0,
        }
    }
}

impl Advancer for Actor {
    fn advance(&mut self) -> ActorState {
        while let Some(mut event) = self.merger.next() {
            //println!("{}: {:?}", self.id, event);

            if event.time > self.time_limit {
                break;
            }

            let mut rng = rand::thread_rng();
            match event.event_type {
                EventType::Close => unreachable!(),
                EventType::Null => unreachable!(),
                EventType::Stalled => {
                    for dst_ix in 0..self.out_times.len() {
                        let out_time = self.out_times[dst_ix];
                        // equal because they might just need a jog, blocking happens in the
                        // iterator, so no infinite loop risk
                        if out_time < event.time {
                            //let cur_time = std::cmp::max(event.time, out_time);
                            self.out_queues[dst_ix]
                                .push(Event {
                                    event_type: EventType::Null,
                                    src: self.id,
                                    time: event.time + LOOKAHEAD,
                                })
                                .unwrap();
                            //self.count += 1;

                            self.out_times[dst_ix] = event.time;
                        }
                    }

                    return ActorState::Continue;
                }
                EventType::ModelEvent(_) => {
                    self.count += 1;
                    // pick a destination, time
                    let dst_ix = self.unif.sample(&mut rng);

                    let cur_time = std::cmp::max(self.out_times[dst_ix], event.time);
                    let dst_time = cur_time + 10 * T_MULT;

                    //event.src = self.id;
                    event.time = dst_time + LOOKAHEAD;

                    // send event
                    self.out_queues[dst_ix].push(event).unwrap();
                    self.out_times[dst_ix] = dst_time;
                }
            }
        }

        ActorState::Done(self.count)
    }
}

/// Transposes incoming rectangular 2d array
///
/// # Examples
/// ```
/// use rustasim::phold::transpose;
/// let v = vec![vec![1, 2, 3], vec![4, 5, 6]];
/// let t = transpose(v);
///
/// let expected = vec![vec![1, 4], vec![2, 5], vec![3, 6]];
/// assert_eq!(t, expected);
/// ```
pub fn transpose<T>(in_vector: Vec<Vec<T>>) -> Vec<Vec<T>> {
    let mut result: Vec<Vec<T>> = Vec::new();

    // initialize the columns
    for _ in 0..in_vector[0].len() {
        result.push(Vec::new());
    }

    for mut col in in_vector {
        for (i, element) in col.drain(..).enumerate() {
            result[i].push(element);
        }
    }

    result
}

pub fn run(n_actors: usize, mut time_limit: u64, n_threads: usize) {
    time_limit *= T_MULT;
    println!("Setup...");

    // Queues
    let mut out_queues = Vec::new();
    let mut in_queues = Vec::new();
    for _ in 0..n_actors {
        // The ins and outs for each actor
        let mut outs = Vec::new();
        let mut ins = Vec::new();

        for _ in 0..n_actors {
            // we need self loops
            let (prod, cons) = spsc::new(Q_SIZE);
            outs.push(prod);
            ins.push(cons);
        }

        out_queues.push(outs);
        in_queues.push(ins);
    }

    // transpose consumers they're on the other end of (src, dst)
    let mut in_queues = transpose(in_queues);

    // Actors
    let mut actors = Vec::new();
    for id in 0..n_actors {
        let outs = out_queues.pop().unwrap();
        let ins = in_queues.pop().unwrap();
        let a = Actor::new(id, outs, ins, time_limit); // TODO
        actors.push(Box::new(a) as Box<dyn Advancer + Send>);
    }

    // Workers

    println!("Run...");
    let start = Instant::now();
    let counts = crate::start(num_cpus::get() - 1, actors);
    let duration = start.elapsed();

    // stats...
    let sum_count = counts.iter().sum::<u64>();
    let ns_per_count: f64 = if sum_count > 0 {
        1000. * duration.as_nanos() as f64 / sum_count as f64
    } else {
        0.
    };

    println!(
        "= {} in {:.3}s. {} actors, {} threads",
        sum_count,
        duration.as_secs_f32(),
        n_actors,
        n_threads,
    );
    println!(
        "  {:.3}M count/sec, {:.3}M /actors, {:.3}M /thread",
        (1e6 / ns_per_count as f64),
        (1e6 / (ns_per_count * n_actors as f64)),
        (1e6 / (ns_per_count * n_threads as f64)),
    );
    println!(
        "  {:.1} ns/count, {:.1} ns/actor, {:.1} ns/thread",
        ns_per_count / 1000. as f64,
        ns_per_count * n_actors as f64 / 1000.,
        ns_per_count * n_threads as f64 / 1000.,
    );

    println!("done");
}
