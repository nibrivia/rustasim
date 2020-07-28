//! Server module

use crate::tcp;
use crate::tcp::Flow;
use crate::tcp::Timeout;
use crate::tcp::MIN_RTO;
use crate::{tx_rx_time, Connectable, ModelEvent, NetworkEvent, Time, Q_SIZE};
use rustasim::spsc;
use rustasim::spsc::*;
use rustasim::{ActorState, Advancer, Event, EventType, Merger};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;

type MinHeap<T> = BinaryHeap<Reverse<T>>;

/// A ServerBuilder is used to create a Server
///
/// Notably, once a server is created, it cannot be modified, the builder however can be changed,
/// connected to other builders and so on.
#[derive(Debug)]
pub struct ServerBuilder {
    /// Future ID of the server
    pub id: usize,

    bandwidth_gbps: u64,
    latency_ns: Time,

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
    /// Starts the process for building a server
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

            bandwidth_gbps: 10,
            latency_ns: 500,

            id_to_ix,
            ix_to_id,
            next_ix: 1,

            in_queues,
            out_queues,
        }
    }

    /// Define server's outgoing bandwidth
    pub fn bandwidth_gbps(mut self, bandwidth_gbps: Time) -> ServerBuilder {
        self.bandwidth_gbps = bandwidth_gbps;
        self
    }
    /// Define server's outgoing latency
    pub fn latency_ns(mut self, latency: Time) -> ServerBuilder {
        self.latency_ns = latency;
        self
    }

    /// Establishes a connection to the "World", see documentation for World
    pub fn connect_world(&mut self) -> Producer<ModelEvent> {
        // world queue
        // TODO create a WORLD_ID thing
        let (world_prod, world_cons) = spsc::new(Q_SIZE);

        self.id_to_ix.insert(0, self.next_ix);
        self.ix_to_id.push(0);
        self.in_queues.push(world_cons);

        world_prod
    }

    /// Returns the Server with the specified parameters
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
                event_type: EventType::ModelEvent(NetworkEvent::Timeout),
                src: self.id,
                time: MIN_RTO,
            })
            .unwrap();

        Server {
            id: self.id,

            bandwidth_gbps: self.bandwidth_gbps,
            latency_ns: self.latency_ns,

            out_queues: self.out_queues,

            merger,

            _ix_to_id: self.ix_to_id,

            tor_time: 0,
            timeouts: MinHeap::new(),
            count: 0,

            flows: Vec::new(),
        }
    }
}

/// Server-in-a-rack actor
///
/// The server has 3 neighbours: the top-of-rack switch, the outside world, and itself (for
/// timeouts). Timeouts are particularly tricky in that they might not be monotonically
/// scheduled... TBD
#[derive(Debug)]
pub struct Server {
    /// Unique ID for the server
    pub id: usize,

    bandwidth_gbps: u64,
    latency_ns: Time,

    merger: Merger<Time, NetworkEvent>,
    out_queues: Vec<Producer<ModelEvent>>,

    _ix_to_id: Vec<usize>,

    tor_time: Time,
    timeouts: MinHeap<Timeout>,

    flows: Vec<tcp::Flow>,

    count: u64,
}

impl Server {
    /// Starts the server, consuming it
    ///
    /// The return value is a counter of some sort. It is mostly used for fast stats on the run.
    /// This will almost certainly change to a function with no return value in the near future.
    pub fn start(&mut self) -> u64 {
        println!(" Server {} start", self.id);
        while let ActorState::Continue(_) = self.advance() {}

        println!(" Server {} done", self.id);
        self.count
    }
}

impl Advancer<Time, u64> for Server {
    fn advance(&mut self) -> ActorState<Time, u64> {
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
                    for out_q in self.out_queues.iter() {
                        out_q
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
                    }

                    // We're stalled, return so that we can be rescheduled later
                    //println!(" Server {} stall", self.id);
                    return ActorState::Continue(event.time);
                }

                EventType::Null => {} //unreachable!(),

                EventType::ModelEvent(net_event) => {
                    self.count += 1;
                    // both of these might schedule packets and timeouts
                    let (packets, timeouts) = match net_event {
                        // TIMEOUT ==============================
                        NetworkEvent::Timeout => {
                            // TODO process ties in one go?
                            // See if we can process any timeouts
                            let mut res = (vec![], vec![]);
                            if let Some(Reverse((t, flow_id, seq_num))) = self.timeouts.peek() {
                                // process (should always be == or >)
                                if *t <= event.time {
                                    // Get packets and timeout to send
                                    //print!("@{} ", event.time);
                                    res = self.flows.get_mut(*flow_id).unwrap().timeout(*seq_num);

                                    // advance the heap
                                    self.timeouts.pop();
                                }
                            }

                            // Schedule next timeout, default min_rto
                            let mut timeout_event = Event {
                                event_type: EventType::ModelEvent(NetworkEvent::Timeout),
                                src: self.id,
                                time: event.time + MIN_RTO,
                            };

                            // or next timeout if there's one before then...
                            if let Some(Reverse((t, _, _))) = self.timeouts.peek() {
                                if *t < timeout_event.time {
                                    timeout_event.time = *t;
                                }
                            }

                            // actually schedule the timeout
                            self.out_queues[0].push(timeout_event).unwrap();

                            // return our packets
                            res
                        }

                        // FLOW =================================
                        NetworkEvent::Flow((src, dst, size_byte)) => {
                            // create flow
                            let flow_id = self.flows.len();
                            let mut flow = Flow::new(flow_id, src, dst, size_byte);

                            // get first group of packets to return later
                            let start = flow.start(event.time);

                            // add to our book-keeping
                            self.flows.insert(flow.flow_id, flow);

                            // return first packet/timeouts
                            start
                        }

                        // PACKET ===============================
                        NetworkEvent::Packet(mut packet) => {
                            if packet.is_ack {
                                let flow = self.flows.get_mut(packet.flow_id).unwrap();
                                flow.src_receive(event.time, packet)
                            } else {
                                // this is data, send ack back
                                packet.dst = packet.src;
                                packet.src = self.id;

                                packet.is_ack = true;
                                packet.size_byte = 10; // TODO parametrize

                                // since we're only sending one packet, no timeout, skip to the next event
                                let (tx_end, rx_end) = tx_rx_time(
                                    self.tor_time,
                                    packet.size_byte,
                                    self.latency_ns,
                                    self.bandwidth_gbps,
                                );

                                tor_q
                                    .push(Event {
                                        event_type: EventType::ModelEvent(NetworkEvent::Packet(
                                            packet,
                                        )),
                                        src: self.id,
                                        time: rx_end,
                                    })
                                    .unwrap();

                                self.tor_time = tx_end;
                                continue;
                            }
                        }
                    };

                    // send the packets
                    let mut tx_end = self.tor_time;
                    for p in packets {
                        /*let (tx_end, rx_end) = tx_rx_time(
                            self.tor_time,
                            p.size_byte,
                            self.latency_ns,
                            self.bandwidth_gbps,
                        );*/
                        tx_end += self.bandwidth_gbps * p.size_byte * 8;
                        let rx_end = tx_end + self.latency_ns;

                        let event = Event {
                            event_type: EventType::ModelEvent(NetworkEvent::Packet(p)),
                            src: self.id,
                            time: rx_end,
                        };

                        tor_q.push(event).unwrap();
                    }

                    self.tor_time = tx_end;

                    // schedule the timeouts
                    for (delay, flow_id, seq_num) in timeouts {
                        self.timeouts
                            .push(Reverse((event.time + delay, flow_id, seq_num)));
                    }
                }
            }
        }

        ActorState::Done(self.count)
    }
}
