//! Deals with most of the IP-layerish things: routing, matching packets to flows...

use crossbeam::queue::spsc;
use crossbeam::queue::spsc::*;
use std::collections::HashMap;
use std::fmt;

use crate::engine::*;
use crate::network::tcp;
use crate::network::ModelEvent;
use crate::network::NetworkEvent;

/// Device types
pub enum Device {
    Router,
    Server,
}

/// A standard interface for connecting devices of all types
// TODO change this API, connect(a, b) function, connectable just has functions for giving and
// getting queues.
pub trait Connectable {
    fn id(&self) -> usize;
    fn flavor(&self) -> Device;

    fn connect(&mut self, other: impl Connectable);
    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent>;
}

/// Top of rack switch
///
/// Connects down to a certain number of servers and out to backbone switches. It is important that
/// the up- and down- bandwidth be matched lest there be excessive queues.
///
/// For performance reasons, it is beneficial to not use hash tables in critical-path data
/// structures. This means that each `Router` has a mapping of other Router IDs to an index. `Event`s
/// coming out of the `Merger` already have their `src` field converted to the right index for us.
pub struct Router {
    id: usize,

    // fundamental properties
    latency_ns: u64,
    ns_per_byte: u64,

    id_to_ix: HashMap<usize, usize>,
    ix_to_id: HashMap<usize, usize>,
    next_ix: usize,

    // event management
    //event_receiver: EventScheduler,
    in_queues: Vec<Consumer<ModelEvent>>,
    out_queues: Vec<Producer<ModelEvent>>,
    out_times: Vec<u64>,

    // networking things
    route: Vec<usize>,

    // stats
    count: u64,
}

impl fmt::Display for Router {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Router {}", self.id)
    }
}

impl Connectable for &mut Router {
    fn id(&self) -> usize {
        self.id
    }

    fn flavor(&self) -> Device {
        Device::Router
    }

    fn connect(&mut self, mut other: impl Connectable) {
        let (prod, cons) = spsc::new(128);

        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.insert(self.next_ix, other.id());

        let tx_queue = (other).back_connect(&mut **self, prod);
        self.out_queues.push(tx_queue);
        self.in_queues.push(cons);
        self.out_times.push(0);

        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        self.next_ix += 1;
    }

    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent> {
        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.insert(self.next_ix, other.id());

        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        let (prod, cons) = spsc::new(128);
        self.in_queues.push(cons);

        self.next_ix += 1;

        prod
    }
}

impl Router {
    pub fn new(id: usize) -> Router {
        Router {
            id,
            latency_ns: 100,
            ns_per_byte: 1,

            id_to_ix: HashMap::new(),
            ix_to_id: HashMap::new(),
            next_ix: 0,

            //event_receiver: EventScheduler::new(id),
            in_queues: Vec::new(),
            out_queues: Vec::new(),
            out_times: Vec::new(),
            //out_notify : HashMap::new(),
            route: Vec::new(),

            count: 0,
        }
    }

    pub fn init_queue(&mut self, dst: usize, events: Vec<ModelEvent>) {
        let dst_ix = self.id_to_ix[&dst];
        for e in events {
            self.out_queues[dst_ix].push(e).unwrap();
        }
    }

    // needs to be called last
    pub fn connect_world(&mut self) -> Producer<ModelEvent> {
        self.id_to_ix.insert(0, self.next_ix);

        let (prod, cons) = spsc::new(128);
        self.in_queues.push(cons);

        prod
    }

    pub fn start(mut self) -> u64 {
        let merger = Merger::new(self.in_queues);

        // TODO auto route
        // route is an id->ix structure
        for dst in 0..41 {
            if self.id_to_ix.contains_key(&dst) {
                self.route.push(self.id_to_ix[&dst]);
            } else {
                self.route.push(0);
            }
        }

        println!("Router #{} starting...", self.id);

        for event in merger {
            self.count += 1;
            match event.event_type {
                EventType::Close => {
                    // ensure everyone ignores us from now until close
                    for dst_ix in 0..self.out_queues.len() {
                        self.out_queues[dst_ix]
                            .push(Event {
                                event_type: EventType::Close,
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
                    for dst_ix in 0..self.out_times.len() {
                        let out_time = self.out_times[dst_ix];
                        // equal because they might just need a jog, blocking happens in the
                        // iterator, so no infinite loop risk
                        if out_time <= event.time {
                            //let cur_time = std::cmp::max(event.time, out_time);
                            self.out_queues[dst_ix]
                                .push(Event {
                                    event_type: EventType::Null,
                                    src: self.id,
                                    time: event.time + self.latency_ns,
                                })
                                .unwrap();
                            //self.count += 1;

                            self.out_times[dst_ix] = event.time;
                        }
                    }
                }

                // This is a message from neighbour we were waiting on, it has served its purpose
                EventType::Null => {
                    //println!("@{} Router #{} got null from #{}",
                    //event.time, self.id, self.ix_to_id[&event.src]);
                    unreachable!();
                }

                EventType::ModelEvent(model_event) => {
                    match model_event {
                        // this is only for servers, not routers
                        NetworkEvent::Flow(_flow) => unreachable!(),

                        NetworkEvent::Packet(packet) => {
                            //self.count += 1;
                            println!(
                                "\x1b[0;3{}m@{} Router {} received {:?} from {}\x1b[0;00m",
                                self.id + 1,
                                event.time,
                                self.id,
                                packet,
                                event.src
                            );
                            /*if packet.dst == self.id {
                                // bounce!
                                packet.dst = packet.src;
                                packet.src = self.id;
                            }*/

                            // TODO who
                            let next_hop_ix: usize = if packet.dst == 99 {
                                if self.id == 1 {
                                    self.id_to_ix[&99]
                                } else {
                                    self.id_to_ix[&1]
                                }
                            } else {
                                if self.id == 3 {
                                    self.id_to_ix[&100]
                                } else {
                                    self.id_to_ix[&3]
                                }
                            };

                            //let next_hop_ix = self.route[packet.dst];
                            //let next_hop_ix = self.id_to_ix[&packet.dst];
                            println!(
                                "\x1b[0;3{}m@{} Router {} received {:?} from {} ->{} [{}]\x1b[0;00m",
                                self.id + 1,
                                event.time,
                                self.id,
                                packet,
                                event.src,
                                self.ix_to_id[&next_hop_ix],
                                next_hop_ix,
                            );

                            // when
                            let cur_time = std::cmp::max(event.time, self.out_times[next_hop_ix]);
                            let tx_end = cur_time + self.ns_per_byte * packet.size_byte;
                            let rx_end = tx_end + self.latency_ns;

                            //println!("\x1b[0;3{}m@{} Router {} sent {:?} to {}@{}",
                            //self.id+1, event.time, self.id, packet, next_hop, rx_end);
                            // go
                            if let Err(e) = self.out_queues[next_hop_ix].push(Event {
                                event_type: EventType::ModelEvent(NetworkEvent::Packet(packet)),
                                src: self.id,
                                time: rx_end,
                            }) {
                                println!(
                                    "@{} Router #{} push error to #{}: {:?}",
                                    event.time, self.id, self.ix_to_id[&next_hop_ix], e
                                );
                                break;
                            }

                            // do this after we send the event over
                            self.out_times[next_hop_ix] = tx_end;
                        } // end EventType::packet
                    }
                }
            } // end match
        } // end for loop

        println!("Router #{} done. {} pkts", self.id, self.count);
        self.count
    } // end start() function
} // end NIC methods

/// Server-in-a-rack actor
///
/// The server has 3 neighbours: the top-of-rack switch, the outside world, and itself (for
/// timeouts). Timeouts are particularly tricky in that they might not be monotonically
/// scheduled... TBD
pub struct Server {
    id: usize,

    ns_per_byte: u64,
    latency_ns: u64,

    id_to_ix: HashMap<usize, usize>,
    ix_to_id: HashMap<usize, usize>,
    next_ix: usize,

    in_queues: Vec<Consumer<ModelEvent>>,
    out_queues: Vec<Producer<ModelEvent>>,

    //tor_link: Producer<ModelEvent>,
    //world_link: Producer<ModelEvent>,
    //self_link: Producer<Event<ModelEvent>>,
    flows: HashMap<usize, tcp::Flow>,

    count: u64,
}

impl Connectable for &mut Server {
    fn id(&self) -> usize {
        self.id
    }

    fn flavor(&self) -> Device {
        Device::Server
    }

    fn connect(&mut self, mut other: impl Connectable) {
        let (prod, cons) = spsc::new(128);

        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.insert(self.next_ix, other.id());

        let tx_queue = (other).back_connect(&mut **self, prod);
        self.out_queues.push(tx_queue);
        self.in_queues.push(cons);

        self.next_ix += 1;
    }

    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent> {
        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.insert(self.next_ix, other.id());

        self.out_queues.push(tx_queue);

        let (prod, cons) = spsc::new(128);
        self.in_queues.push(cons);

        self.next_ix += 1;

        prod
    }
}

impl Server {
    pub fn new(id: usize) -> Server {
        let mut id_to_ix = HashMap::new();
        let mut ix_to_id = HashMap::new();

        let mut in_queues = Vec::new();
        let mut out_queues = Vec::new();

        let mut out_times = Vec::new();

        // self queue
        let (self_prod, self_cons) = spsc::new(128);

        id_to_ix.insert(id, 0);
        ix_to_id.insert(0, id);
        in_queues.push(self_cons);
        out_queues.push(self_prod);

        out_times.push(0);

        Server {
            id,

            ns_per_byte: 1,
            latency_ns: 100,

            id_to_ix,
            ix_to_id,
            next_ix: 1,

            in_queues,
            out_queues,

            flows: HashMap::new(),

            count: 0,
        }
    }

    pub fn connect_world(&mut self) -> Producer<ModelEvent> {
        // world queue
        // TODO create a WORLD_ID thing
        let (world_prod, world_cons) = spsc::new(128);

        self.id_to_ix.insert(0, self.next_ix);
        self.ix_to_id.insert(self.next_ix, 0);
        self.in_queues.push(world_cons);

        world_prod
    }

    pub fn start(mut self) -> u64 {
        // FIXME timeouts not yet implemented, let's keep this channel inactive
        self.out_queues[0]
            .push(Event {
                event_type: EventType::Close,
                src: self.id,
                time: 1_000_000_000_000_000, // large value
            })
            .unwrap();

        println!("Server #{} starting... {:?}", self.id, self.id_to_ix);

        let mut tor_time = 0;
        let tor_q = &self.out_queues[1];
        //let mut self_time = 0;

        let merger = Merger::new(self.in_queues);
        for event in merger {
            self.count += 1;
            //println!("@{} event #{}", event.time, self.count);
            match event.event_type {
                EventType::Close => {
                    // ensure everyone ignores us from now until close
                    for dst_ix in 0..self.out_queues.len() {
                        self.out_queues[dst_ix]
                            .push(Event {
                                event_type: EventType::Close,
                                src: self.id,
                                time: event.time + self.latency_ns,
                            }) // add latency to avoid violating in-order invariant
                            .unwrap();
                    }

                    break;
                }

                EventType::Stalled => {
                    // TODO how on earth do we tell ourselves to move forward??
                    // min timeout of 100us
                    /*
                    if self_time <= event.time {
                        //let cur_time = std::cmp::max(event.time, out_time);
                        self.out_queues[0]
                            .push(Event {
                                event_type: EventType::Null,
                                src: self.id,
                                time: event.time + 10_000,
                            })
                            .unwrap();
                        //self.count += 1;

                        self.out_times[0] = event.time;
                    }
                    */

                    // ToR
                    // equal because they might just need a jog, blocking happens in the
                    // iterator, so no infinite loop risk
                    if tor_time <= event.time {
                        //let cur_time = std::cmp::max(event.time, out_time);
                        tor_q
                            .push(Event {
                                event_type: EventType::Null,
                                src: self.id,
                                time: event.time + self.latency_ns,
                            })
                            .unwrap();
                        //self.count += 1;

                        tor_time = event.time;
                    }
                }

                EventType::Null => unreachable!(),

                EventType::ModelEvent(net_event) => {
                    // both of these might schedule packets and timeouts
                    let (packets, timeouts) = match net_event {
                        NetworkEvent::Flow(mut flow) => {
                            println!("flow");
                            let start = flow.start();
                            self.flows.insert(flow.flow_id, flow);
                            start
                        }

                        NetworkEvent::Packet(mut packet) => {
                            println!("@{} Server #{} rx packet {:?}", event.time, self.id, packet);

                            if !packet.is_ack {
                                // this is data, send ack back
                                packet.dst = packet.src;
                                packet.src = self.id;

                                packet.is_ack = true;
                                packet.size_byte = 10;

                                (vec![packet], vec![])
                            } else {
                                // this is an ACK
                                let flow = self.flows.get_mut(&packet.flow_id).unwrap();
                                flow.src_receive(packet)
                            }
                        }
                    };

                    // send the packets
                    for p in packets {
                        let tx_end = tor_time + self.ns_per_byte * p.size_byte;
                        let rx_end = tx_end + self.latency_ns;

                        // actually send the packet :)
                        tor_q
                            .push(Event {
                                event_type: EventType::ModelEvent(NetworkEvent::Packet(p)),
                                src: self.id,
                                time: rx_end,
                            })
                            .unwrap();

                        // TODO move out of loop?
                        tor_time = tx_end;
                    }

                    // TODO schedule the timeouts
                    for _ in timeouts {
                        // self_q.push(...)
                    }
                }
            }
        }

        println!("Server #{} done. {} pkts", self.id, self.count);
        self.count
    }
}
