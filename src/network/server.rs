use crossbeam_queue::spsc;
use crossbeam_queue::spsc::*;
use std::collections::HashMap;

use crate::engine::*;
use crate::network::tcp;
use crate::network::{Connectable, Device, ModelEvent, NetworkEvent, Q_SIZE};
use crate::worker::{ActorState, Advancer};

pub struct ServerBuilder {
    pub id: usize,

    ns_per_byte: u64,
    latency_ns: u64,

    id_to_ix: HashMap<usize, usize>,
    ix_to_id: Vec<usize>,
    next_ix: usize,

    in_queues: Vec<Consumer<ModelEvent>>,
    out_queues: Vec<Producer<ModelEvent>>,
}

impl Connectable for &mut ServerBuilder {
    fn id(&self) -> usize {
        self.id
    }

    fn flavor(&self) -> Device {
        Device::Server
    }

    fn connect(&mut self, mut other: impl Connectable) {
        let (prod, cons) = spsc::new(Q_SIZE);

        self.id_to_ix.insert(other.id(), self.next_ix);
        self.ix_to_id.push(other.id());

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
        self.ix_to_id.push(other.id());

        self.out_queues.push(tx_queue);

        let (prod, cons) = spsc::new(Q_SIZE);
        self.in_queues.push(cons);

        self.next_ix += 1;

        prod
    }
}

impl ServerBuilder {
    // TODO document
    pub fn new(id: usize) -> ServerBuilder {
        let mut id_to_ix = HashMap::new();
        let mut ix_to_id = Vec::new();

        let mut in_queues = Vec::new();
        let mut out_queues = Vec::new();

        let mut out_times = Vec::new();

        // self queue
        let (self_prod, self_cons) = spsc::new(Q_SIZE);

        id_to_ix.insert(id, 0);
        ix_to_id.insert(0, id);
        in_queues.push(self_cons);
        out_queues.push(self_prod);

        out_times.push(0);

        ServerBuilder {
            id,

            ns_per_byte: 1,
            latency_ns: 100,

            id_to_ix,
            ix_to_id,
            next_ix: 1,

            in_queues,
            out_queues,
        }
    }

    // TODO document
    pub fn connect_world(&mut self) -> Producer<ModelEvent> {
        // world queue
        // TODO create a WORLD_ID thing
        let (world_prod, world_cons) = spsc::new(Q_SIZE);

        self.id_to_ix.insert(0, self.next_ix);
        self.ix_to_id.push(0);
        self.in_queues.push(world_cons);

        world_prod
    }

    pub fn build(self) -> Server {
        let mut v = Vec::new();
        for id in &self.ix_to_id {
            v.push(*id);
        }

        let merger = Merger::new(self.in_queues, self.id, v);

        // Send null events to the ToR
        self.out_queues[1]
            .push(Event {
                event_type: EventType::Null,
                src: self.id,
                time: self.latency_ns,
            })
            .unwrap();

        // null event to ourselves...
        self.out_queues[0]
            .push(Event {
                event_type: EventType::Null,
                src: self.id,
                time: 1_000_000_000_000_000,
            })
            .unwrap();

        Server {
            id: self.id,

            ns_per_byte: self.ns_per_byte,
            latency_ns: self.latency_ns,

            out_queues: self.out_queues,

            merger,

            _ix_to_id: self.ix_to_id,

            tor_time: 0,
            count: 0,

            flows: HashMap::new(),
        }
    }
}

/// Server-in-a-rack actor
///
/// The server has 3 neighbours: the top-of-rack switch, the outside world, and itself (for
/// timeouts). Timeouts are particularly tricky in that they might not be monotonically
/// scheduled... TBD
pub struct Server {
    pub id: usize,

    ns_per_byte: u64,
    latency_ns: u64,

    merger: Merger<NetworkEvent>,
    out_queues: Vec<Producer<ModelEvent>>,

    _ix_to_id: Vec<usize>,

    tor_time: u64,

    flows: HashMap<usize, tcp::Flow>,

    pub count: u64,
}

impl Server {
    /// Starts the server, consuming it
    ///
    /// The return value is a counter of some sort. It is mostly used for fast stats on the run.
    /// This will almost certainly change to a function with no return value in the near future.
    pub fn start(&mut self) -> u64 {
        println!(" Server {} start", self.id);
        while let ActorState::Continue = self.advance() {}

        println!(" Server {} done", self.id);
        return self.count;
    }
}

impl Advancer for Server {
    //pub fn advance(&mut self, log: slog::Logger, start: Instant) -> bool {
    fn advance(&mut self) -> ActorState {
        //info!(log, "start...");
        //println!(" Server {} advance", self.id);

        let tor_q = &self.out_queues[1];

        // TODO figure out this whole loop thing?
        //for event in self.merger {
        while let Some(event) = self.merger.next() {
            //self.count += 1;
            /*println!(
                " Server {} @{}: <{} {:?}",
                self.id, event.time, self._ix_to_id[event.src], event.event_type
            );*/
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
                    if self.tor_time < event.time {
                        tor_q
                            .push(Event {
                                event_type: EventType::Null,
                                src: self.id,
                                time: event.time + self.latency_ns,
                            })
                            .unwrap();
                        //self.count += 1;

                        self.tor_time = event.time;
                        /*println!(
                            " Server {} @{}: Null({}) >{}",
                            self.id,
                            event.time,
                            event.time + self.latency_ns,
                            self._ix_to_id[1]
                        );*/
                    }

                    // We're stalled, return so that we can be rescheduled later
                    //println!(" Server {} stall", self.id);
                    return ActorState::Continue;
                }

                EventType::Null => {} //unreachable!(),

                EventType::ModelEvent(net_event) => {
                    self.count += 1;
                    // both of these might schedule packets and timeouts
                    let (packets, timeouts) = match net_event {
                        NetworkEvent::Flow(mut flow) => {
                            let start = flow.start();
                            self.flows.insert(flow.flow_id, flow);
                            start
                        }

                        NetworkEvent::Packet(mut packet) => {
                            if packet.is_ack {
                                let flow = self.flows.get_mut(&packet.flow_id).unwrap();
                                flow.src_receive(packet)
                            } else {
                                // this is data, send ack back
                                packet.dst = packet.src;
                                packet.src = self.id;

                                packet.is_ack = true;
                                packet.size_byte = 10;

                                (vec![packet], vec![])
                            }
                        }
                    };

                    // send the packets
                    for p in packets {
                        let tx_end = self.tor_time + self.ns_per_byte * p.size_byte;
                        let rx_end = tx_end + self.latency_ns;

                        let event = Event {
                            event_type: EventType::ModelEvent(NetworkEvent::Packet(p)),
                            //real_time: start.elapsed().as_nanos(),
                            //real_time: 0,
                            src: self.id,
                            time: rx_end,
                        };

                        // actually send the packet :)
                        tor_q.push(event).unwrap();

                        // TODO move out of loop?
                        self.tor_time = tx_end;
                    }

                    // TODO schedule the timeouts
                    for _ in timeouts {
                        // self_q.push(...)
                    }
                }
            }
        }

        //info!(log, "Server #{} done. {} pkts", self.id, self.count);
        ActorState::Done(self.count)
    }
}
