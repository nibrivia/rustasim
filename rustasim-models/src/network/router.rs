//! Router module, takes care of ToRs and backbone switches

use crate::network::{Connectable, Device, ModelEvent, NetworkEvent, Q_SIZE};
use rand::seq::SliceRandom;
use rand::thread_rng;
use rustasim::engine::*;
use rustasim::spsc;
use rustasim::spsc::*;
use rustasim::worker::{ActorState, Advancer};
use std::collections::HashMap;

/// Top of rack switch builder
///
/// Connects down to a certain number of servers and out to backbone switches. It is important that
/// the up- and down- bandwidth be matched lest there be excessive queues.
#[derive(Debug)]
pub struct RouterBuilder {
    /// ID of the Router to be built
    pub id: usize,

    // fundamental properties
    latency_ns: u64,
    ns_per_byte: u64,

    // internal mappings
    id_to_ix: HashMap<usize, usize>,
    ix_to_id: Vec<usize>,
    next_ix: usize,

    // route
    route: Vec<Vec<usize>>,

    // event management
    in_queues: Vec<Consumer<ModelEvent>>,
    out_queues: Vec<Producer<ModelEvent>>,
}

impl Connectable for &mut RouterBuilder {
    fn id(&self) -> usize {
        self.id
    }

    fn flavor(&self) -> Device {
        Device::Router
    }

    fn connect(&mut self, mut other: impl Connectable) {
        let (prod, cons) = spsc::new(Q_SIZE);

        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.push(other.id());

        let tx_queue = (other).back_connect(&mut **self, prod);
        self.out_queues.push(tx_queue);
        self.in_queues.push(cons);
        //self.out_times.push(0);

        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        self.next_ix += 1;
    }

    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent> {
        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.push(other.id());

        self.out_queues.push(tx_queue);
        //self.out_times.push(0);
        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        let (prod, cons) = spsc::new(Q_SIZE);
        self.in_queues.push(cons);

        self.next_ix += 1;

        prod
    }
}

impl RouterBuilder {
    /// Creates a new RouterBuilder with sensible defaults
    pub fn new(id: usize) -> RouterBuilder {
        RouterBuilder {
            id,
            latency_ns: 500,
            ns_per_byte: 1,

            id_to_ix: HashMap::new(),
            ix_to_id: Vec::new(),
            next_ix: 0,

            in_queues: Vec::new(),
            out_queues: Vec::new(),

            route: Vec::new(),
        }
    }

    // needs to be called last
    /// Creates a connection the World, see documentation for World
    pub fn connect_world(&mut self) -> Producer<ModelEvent> {
        self.id_to_ix.insert(0, self.next_ix);

        let (prod, cons) = spsc::new(Q_SIZE);
        self.in_queues.push(cons);
        self.ix_to_id.push(0);

        prod
    }

    /// Installs an externally computed routing table
    ///
    /// **This function assumes that IDs start at 1 and are continuous from there.**
    ///
    /// The routing table should specify for each ID what is the ID of the next hop. There is no
    /// requirement for the next ID for the device's own ID.
    ///
    /// The motivation for an external routing is that it is significantly simpler than
    /// implementing a distributed routing algorithm. As the research might become more specific to
    /// routing, this function may loose its purpose
    pub fn install_routes(&mut self, routes: HashMap<usize, Vec<usize>>) {
        // skip 0
        self.route = vec![vec![]];

        // have to index in-order
        for dst_id in 1..routes.len() + 1 {
            // skip self
            if dst_id == self.id {
                continue;
            }

            let next_hop_ids = &routes[&dst_id];
            let next_hop_ixs = next_hop_ids.iter().map(|x| self.id_to_ix[&x]).collect();

            self.route.push(next_hop_ixs);
        }
    }

    /// Builds the server described by this builder
    pub fn build(self) -> Router {
        // build the event merger
        let mut v = Vec::new();
        for id in &self.ix_to_id {
            v.push(*id);
        }

        let merger = Merger::new(self.in_queues, self.id, v);

        let mut out_times = vec![];
        for dst_ix in 0..self.out_queues.len() {
            self.out_queues[dst_ix]
                .push(Event {
                    event_type: EventType::Null,
                    //real_time: start.elapsed().as_nanos(),
                    //real_time: 0,
                    src: self.id,
                    time: self.latency_ns,
                })
                .unwrap();

            out_times.push(0);
        }

        Router {
            id: self.id,

            latency_ns: self.latency_ns,
            ns_per_byte: self.ns_per_byte,

            merger,

            ix_to_id: self.ix_to_id,

            // event management
            out_queues: self.out_queues,
            out_times,

            // Route should eventually be turned into a vec
            route: self.route,

            // stats
            count: 0,
        }
    }
}

/// Top of rack switch
///
/// For performance reasons, it is beneficial to not use hash tables in critical-path data
/// structures. This means that each `Router` has a mapping of other Router IDs to an index. `Event`s
/// coming out of the `Merger` already have their `src` field converted to the right index for us.
#[derive(Debug)]
pub struct Router {
    /// Unique ID of the router
    pub id: usize,

    // fundamental properties
    latency_ns: u64,
    ns_per_byte: u64,

    ix_to_id: Vec<usize>,

    merger: Merger<u64, NetworkEvent>,

    // event management
    out_queues: Vec<Producer<ModelEvent>>,
    out_times: Vec<u64>,

    // Route should eventually be turned into a vec
    route: Vec<Vec<usize>>,

    // stats
    count: u64,
}

impl Router {
    /// Starts the router, will not return until finished
    pub fn start(&mut self) -> u64 {
        println!("Router {} start", self.id);
        while let ActorState::Continue(_) = self.advance() {}

        println!("Router {} done", self.id);
        self.count
    }
}

impl Advancer<u64, u64> for Router {
    /// Starts the rack, consumes the object
    ///
    /// The return value is a counter of some sort. It is mostly used for fast stats on the run.
    /// This will almost certainly change to a function with no return value in the near future.
    //pub fn start(&mut self, log: slog::Logger, start: Instant) -> u64 {
    fn advance(&mut self) -> ActorState<u64, u64> {
        //println!("Router {} advancing", self.id);
        //let log = log.new(o!("Router" => self.id));
        //info!(log, "start...");

        // main loop :)
        //for event in self.merger {
        let mut rng = thread_rng();
        while let Some(event) = self.merger.next() {
            /*println!(
                "Router {} @{}: <{} {:?}",
                self.id, event.time, self.ix_to_id[event.src], event.event_type
            );*/
            //self.count += 1;
            match event.event_type {
                EventType::Close => {
                    // ensure everyone ignores us from now until close
                    for dst_ix in 0..self.out_queues.len() {
                        self.out_queues[dst_ix]
                            .push(Event {
                                event_type: EventType::Close,
                                //real_time: start.elapsed().as_nanos(),
                                src: self.id,
                                time: event.time + self.latency_ns,
                            }) // add latency to avoid violating in-order invariant
                            .unwrap();
                    }

                    break;
                }

                // We're waiting on a neighbour...
                EventType::Stalled => {
                    // We need the time from these friendos
                    for (dst_ix, out_time) in self.out_times.iter_mut().enumerate() {
                        // equal because they might just need a jog, blocking happens in the
                        // iterator, so no infinite loop risk
                        if *out_time < event.time {
                            self.out_queues[dst_ix]
                                .push(Event {
                                    event_type: EventType::Null,
                                    src: self.id,
                                    time: event.time + self.latency_ns,
                                })
                                .unwrap();
                            //self.count += 1;

                            *out_time = event.time;
                        }
                    }

                    return ActorState::Continue(event.time);
                }

                // This is a message from neighbour we were waiting on, it has served its purpose
                EventType::Null => {} //unreachable!(),

                EventType::ModelEvent(model_event) => {
                    self.count += 1;
                    match model_event {
                        // this is only for servers, not routers
                        NetworkEvent::Flow(_flow) => unreachable!(),

                        NetworkEvent::Packet(packet) => {
                            // Next step
                            let next_hop_ix: usize =
                                *self.route[packet.dst].choose(&mut rng).unwrap();

                            // drop packet if our outgoing queue is full
                            if event.time
                                > self.out_times[next_hop_ix] + 1000 * 1500 * self.ns_per_byte
                            {
                                //println!("Router {} drop {:?}", self.id, packet);
                                continue;
                            }

                            // when
                            let cur_time = std::cmp::max(event.time, self.out_times[next_hop_ix]);
                            let tx_end = cur_time + self.ns_per_byte * packet.size_byte;
                            let rx_end = tx_end + self.latency_ns;

                            //println!("\x1b[0;3{}m@{} Router {} sent {:?} to {}@{}",
                            //self.id+1, event.time, self.id, packet, next_hop, rx_end);
                            // go
                            if let Err(e) = self.out_queues[next_hop_ix].push(Event {
                                event_type: EventType::ModelEvent(NetworkEvent::Packet(packet)),
                                //real_time: start.elapsed().as_nanos(),
                                src: self.id,
                                time: rx_end,
                            }) {
                                println!(
                                    "@{} Router #{} push error to #{}: {:?}",
                                    event.time, self.id, self.ix_to_id[next_hop_ix], e
                                );
                                break;
                            }

                            // update our estimate of time
                            self.out_times[next_hop_ix] = tx_end;
                        } // end EventType::packet
                    }
                }
            } // end match
        } // end for loop

        //info!(log, "Router #{} done. {} pkts", self.id, self.count);
        ActorState::Done(self.count)
    } // end start() function
} // end NIC methods
