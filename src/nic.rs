use std::fmt;
//use ringbuf::*;
use crossbeam::queue::spsc::*;
use std::collections::HashMap;

use crate::tcp::*;
use crate::synchronizer::*;

pub struct Router {
    id: usize,

    // fundamental properties
    latency_ns: u64,
    ns_per_byte: u64,

    id_to_ix: HashMap<usize, usize>,
    next_ix: usize,

    // event management
    event_receiver: EventScheduler,
    out_queues: Vec<Producer<Event>>,
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


impl Router {
    pub fn new(id: usize) -> Router {
        Router {
            id,
            latency_ns: 10,
            ns_per_byte: 1,

            id_to_ix: HashMap::new(),
            next_ix: 0,

            event_receiver: EventScheduler::new(id),
            out_queues: Vec::new(),
            out_times: Vec::new(),
            //out_notify : HashMap::new(),
            route: Vec::new(),

            count: 0,
        }
    }

    pub fn init_queue(&mut self, dst: usize, events: Vec<Event>) {
        let dst_ix = self.id_to_ix[&dst];
        for e in events {
            self.out_queues[dst_ix].push(e).unwrap();
        }
    }

    pub fn connect(&mut self, other: &mut Self) {
        let inc_channel = self.event_receiver.connect_incoming(self.next_ix);

        self.id_to_ix.insert(other.id, self.next_ix);

        let tx_queue = other._connect(&self, inc_channel);
        self.out_queues.push(tx_queue);
        self.out_times.push(0);

        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        self.next_ix += 1;
    }

    pub fn _connect(&mut self, other: &Self, tx_queue: Producer<Event>) -> Producer<Event> {
        self.id_to_ix.insert(other.id, self.next_ix);

        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        let chan = self.event_receiver.connect_incoming(self.next_ix);

        self.next_ix += 1;
        return chan;
    }

    // needs to be called last
    pub fn connect_world(&mut self) -> Producer<Event> {
        self.id_to_ix.insert(0, self.next_ix);
        return self.event_receiver.connect_incoming(self.next_ix);
    }

    pub fn start(mut self) -> u64 {
        // kickstart stuff up
        for (dst_ix, out_q) in self.out_queues.iter_mut().enumerate() {
            out_q
                .push(Event {
                    event_type: EventType::Update,
                    src: self.id,
                    time: self.latency_ns,
                })
                .unwrap();
            self.out_times[dst_ix] = self.latency_ns;
        }

        // TODO auto route
        // route is an id->ix structure
        for dst in 0..41 {
            if self.id_to_ix.contains_key(&dst) {
                self.route.push(self.id_to_ix[&dst]);
            } else {
                self.route.push(0);
            }
        }

        println!("Router {} starting...", self.id);
        for event in self.event_receiver {
            match event.event_type {
                EventType::Close => break,

                // This comes from below
                EventType::Missing(dsts) => {
                    // We need the time from these friendos
                    for dst_ix in dsts {
                        self.out_queues[dst_ix]
                            .push(Event {
                                event_type: EventType::Update,
                                src: self.id,
                                time: event.time + self.latency_ns,
                            })
                            .unwrap();
                    }
                },

                EventType::Update => {
                    self.out_queues[event.src]
                        .push(Event {
                            event_type: EventType::Response,
                            src: self.id,
                            time: self.out_times[event.src] + self.latency_ns,
                        })
                        .unwrap();
                }

                EventType::Response => {},

                EventType::Flow(flow) => {},

                EventType::Packet(mut packet) => {
                    //println!("\x1b[0;3{}m@{} Router {} received {:?} from {}\x1b[0;00m", self.id+1, event.time, self.id, packet, event.src);
                    self.count += 1;
                    if packet.dst == self.id {
                        // bounce!
                        packet.dst = packet.src;
                        packet.src = self.id;
                    }

                    // who
                    let next_hop_ix = self.route[packet.dst];
                    //let next_hop_ix = self.id_to_ix[&packet.dst];

                    // when
                    let cur_time = std::cmp::max(event.time, self.out_times[next_hop_ix]);
                    let tx_end = cur_time + self.ns_per_byte * packet.size_byte;
                    let rx_end = tx_end + self.latency_ns;

                    //println!("\x1b[0;3{}m@{} Router {} sent {:?} to {}@{}", self.id+1, event.time, self.id, packet, next_hop, rx_end);
                    // go
                    if let Err(_) = self.out_queues[next_hop_ix].push(Event {
                        event_type: EventType::Packet(packet),
                        src: self.id,
                        time: rx_end,
                    }) {
                        break;
                    }

                    // do this after we send the event over
                    self.out_times[next_hop_ix] = tx_end;
                } // boing...
            } // boing...
        } // boing...
        println!("Router {} done {}", self.id, self.count);
        return self.count;
    } // boing...
} // boing...
// splat

type ServerID = (usize,);

struct Server {
    server_id : ServerID,
    tor_link : Producer<Event>,

    time_ns : u64, // is this needed?
    flows : HashMap<usize, Flow>,
}

impl Server {
    fn new(id : usize, tor_link : Producer<Event>) -> Server {
        Server {
            server_id : (id,),
            tor_link,
            time_ns : 0,
            flows : HashMap::new(),
        }
    }

    fn receive_event(&mut self, event : Event) {
        match event.event_type {
            EventType::Flow(mut flow) => {
                self.flows.insert(flow.flow_id, flow);
                // TODO start flow
            }
            EventType::Packet(mut packet) => {
                println!("Received packet {:?}", packet);
            }
            _ => unreachable!(),
        }
        println!("{:?} go", self.server_id);
    }
}
