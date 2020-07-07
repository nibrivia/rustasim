//! Parallel datacenter network simulator
//!
//! Throughout this crate there is a user-backend relationship between the [simulation
//! engine](synchronizer/index.html) and the model. In general, the engine should be agnostic to
//! the type of model being run, and should probably eventually be pulled out into its own crate.

use crossbeam_deque::Worker;
use crossbeam_queue::spsc;
use crossbeam_queue::spsc::*;
use std::collections::HashMap;
//use std::sync::Arc;
use std::thread;

//use slog::*;
//use slog_async;

pub mod engine;
pub mod logger;
pub mod network;
pub mod worker;

use crate::engine::*;
use crate::network::router::{Router, RouterBuilder};
use crate::network::routing::{route_id, Network};
use crate::network::server::{Server, ServerBuilder};
use crate::network::tcp::*;
use crate::network::{Connectable, ModelEvent, NetworkEvent};
use crate::worker::{run, Advancer};

pub struct World {
    /// The actors themselves
    racks: Vec<Router>,
    servers: Vec<Server>,

    /// Communication channels from us (the world) to the actors
    chans: HashMap<usize, Producer<ModelEvent>>,
}

/// Main simulation object.
///
/// This is where the core of the simualtion setup should happen. Notably it has the important
/// tasks of connection actors together, and to give "external" events to these actors.
///
/// Setthing up and running the simulation are done in two phases. This feels like good design, but
/// it is not clear to me why.
impl World {
    /// Sets up a world ready for simulation
    pub fn new(n_racks: usize) -> World {
        // TODO pass as argument
        let servers_per_rack = n_racks - 1;

        // Use to keep track of ID numbers and make them continuous
        let mut next_id = 1;
        let mut network = Network::new();

        // TODO backbone switches

        // Racks -----------------------------------------------
        let mut rack_builders: Vec<RouterBuilder> = Vec::new();

        for _ in 0..n_racks {
            let mut r = RouterBuilder::new(next_id);
            network.insert(next_id, vec![]);

            // connect up with other racks
            for rack2 in rack_builders.iter_mut() {
                // update our network matrix
                network.get_mut(&next_id).unwrap().push(rack2.id());
                network.get_mut(&rack2.id()).unwrap().push(next_id);

                // connect the devices
                (&mut r).connect(rack2);
            }

            next_id += 1;
            rack_builders.push(r);
        }

        // Servers ---------------------------------------------
        let mut server_builders = Vec::new();

        for rack_ix in 0..n_racks {
            for _ in 0..servers_per_rack {
                let mut s = ServerBuilder::new(next_id);
                network.insert(next_id, vec![]);

                // get the parent rack (needs to be done each time, ref is consumed by connect)
                let rack = rack_builders.get_mut(rack_ix).unwrap();

                // update network matrix
                network.get_mut(&next_id).unwrap().push(rack.id());
                network.get_mut(&rack.id()).unwrap().push(next_id);

                // connect devices
                (&mut s).connect(rack);

                // push and consume
                next_id += 1;
                server_builders.push(s);
            }
        }

        // Routing ---------------------------------------------
        for r in rack_builders.iter_mut() {
            let rack_id = r.id();

            let routes = route_id(&network, rack_id);
            r.install_routes(routes);
        }

        // World -----------------------------------------------
        let mut chans = HashMap::new();
        let mut servers = vec![];
        for mut b in server_builders {
            chans.insert(b.id, b.connect_world());
            servers.push(b.build());
        }

        let mut racks = vec![];
        for mut rb in rack_builders {
            chans.insert(rb.id, rb.connect_world());
            racks.push(rb.build());
        }

        // Flows -----------------------------------------------
        let mut flow_id = 0;
        for src_ix in 0..servers.len() {
            let src_id = (&mut servers[src_ix]).id;

            for dst_ix in 0..servers.len() {
                // skip self flows...
                if src_ix == dst_ix {
                    continue;
                }

                let dst_id = (&mut servers[dst_ix]).id;

                let f = Flow::new(flow_id, src_id, dst_id, 100000000);
                flow_id += 1;

                chans[&src_id]
                    .push(Event {
                        src: 0,
                        time: 0,
                        event_type: EventType::ModelEvent(NetworkEvent::Flow(f)),
                    })
                    .unwrap();
            }
        }

        // return world
        World {
            racks,
            servers,
            chans,
        }
    }

    /// Runs this `World`'s simulation up to time `done`.
    ///
    /// This will spawn a thread per actor and wait for all of them to end.
    pub fn start(mut self, num_cpus: usize, done: u64) -> Vec<u64> {
        // Tell everyone when the end is
        for (_, c) in self.chans.iter_mut() {
            c.push(Event {
                time: done,
                //real_time: 0,
                src: 0,
                event_type: EventType::Close,
            })
            .unwrap();
        }

        // TODO think about where this should be
        // logger
        /*
        let log_path = "out.log";
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(log_path)
            .unwrap();

        //let decorator = slog_term::PlainDecorator::new(file);
        //let drain = slog_term::FullFormat::new(decorator).build().fuse();
        //let drain = slog_json::Json::new(file).build().fuse();
        let drain = logger::MsgLogger::new(file);
        //let start = drain.start;

        let drain = drain.fuse();
        let drain = slog_async::Async::new(drain)
            .overflow_strategy(slog_async::OverflowStrategy::Block)
            .build()
            .fuse();

        let log = Logger::root(drain.fuse(), o!());

        info!(log, "rx_time, tx_time, sim_time, id, src, type");
        */

        // Workers
        let mut workers: Vec<Worker<Box<dyn Advancer + Send>>> = Vec::new();
        let mut stealers = Vec::new();
        for _ in 0..num_cpus {
            let w = Worker::new_fifo();
            stealers.push(w.stealer());
            workers.push(w);
        }

        let mut prods = Vec::new();
        let mut cons = Vec::new();
        for _ in 0..num_cpus {
            let (p, c) = spsc::new(128);
            prods.push(p);
            cons.push(c);
        }

        let p = prods.pop().unwrap();
        prods.push(p);

        let mut cur_worker = 0;
        for s in self.servers {
            workers[cur_worker].push(Box::new(s));
            cur_worker = (cur_worker + 1) % num_cpus;
        }
        for r in self.racks {
            workers[cur_worker].push(Box::new(r));
            cur_worker = (cur_worker + 1) % num_cpus;
        }

        let mut handles = Vec::new();
        //let injector = Arc::new(injector);
        let mut i = 0;
        for mut w in workers {
            let mut local_stealers = Vec::new();
            i += 1;
            for j in i..stealers.len() {
                local_stealers.push(stealers[j].clone());
            }
            for j in 0..i {
                local_stealers.push(stealers[j].clone());
            }

            if local_stealers.len() != stealers.len() {
                unreachable!();
            }

            let prev = cons.pop().unwrap();
            let next = prods.pop().unwrap();

            handles.push(
                //let injector = Arc::clone(&injector);
                thread::spawn(move || run(i, &mut w, prev, next, local_stealers)),
            );
        }

        let mut counts = Vec::new();
        for h in handles {
            let local_counts: Vec<u64> = h.join().unwrap();
            counts.extend(local_counts);
        }

        counts
    }
}
