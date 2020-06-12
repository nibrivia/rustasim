use std::fmt;
use crossbeam::queue::spsc::*;
use std::collections::HashMap;
use crossbeam::queue::spsc;

use crate::synchronizer::*;

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
    in_queues : Vec<Consumer<Event>>,
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
            latency_ns: 100,
            ns_per_byte: 1,

            id_to_ix: HashMap::new(),
            ix_to_id: HashMap::new(),
            next_ix: 0,

            //event_receiver: EventScheduler::new(id),
            in_queues : Vec::new(),
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
        let (prod, cons) = spsc::new(128);

        self.id_to_ix.insert(other.id, self.next_ix);
        self.ix_to_id.insert(self.next_ix, other.id);

        let tx_queue = other._connect(&self, prod);
        self.out_queues.push(tx_queue);
        self.in_queues.push(cons);
        self.out_times.push(0);

        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        self.next_ix += 1;
    }

    pub fn _connect(&mut self, other: &Self, tx_queue: Producer<Event>) -> Producer<Event> {
        self.id_to_ix.insert(other.id, self.next_ix);
        self.ix_to_id.insert(self.next_ix, other.id);

        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        let (prod, cons) = spsc::new(128);
        self.in_queues.push(cons);

        self.next_ix += 1;
        return prod;
    }

    // needs to be called last
    pub fn connect_world(&mut self) -> Producer<Event> {
        self.id_to_ix.insert(0, self.next_ix);

        let (prod, cons) = spsc::new(128);
        self.in_queues.push(cons);
        return prod;
    }

    pub fn start(mut self) -> u64 {
        // TODO very hacky...
        let merger = Merger::new(self.in_queues);

        // kickstart stuff up
        for (dst_ix, out_q) in self.out_queues.iter_mut().enumerate() {
            out_q
                .push(Event {
                    event_type: EventType::Null,
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
                },

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
                },

                // This is a message from neighbour we were waiting on, it has served its purpose
                EventType::Null => {
                    //println!("@{} Router #{} got null from #{}",
                    //event.time, self.id, self.ix_to_id[&event.src]);
                    unreachable!();
                },

                EventType::Flow(_flow) => {},

                EventType::Packet(mut packet) => {
                    //self.count += 1;
                    //println!("\x1b[0;3{}m@{} Router {} received {:?} from {}\x1b[0;00m",
                    //self.id+1, event.time, self.id, packet, event.src);
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

                    //println!("\x1b[0;3{}m@{} Router {} sent {:?} to {}@{}",
                    //self.id+1, event.time, self.id, packet, next_hop, rx_end);
                    // go
                    if let Err(e) = self.out_queues[next_hop_ix].push(Event {
                        event_type: EventType::Packet(packet),
                        src: self.id,
                        time: rx_end,
                    }) {
                        println!("@{} Router #{} push error to #{}: {:?}",
                            event.time, self.id, self.ix_to_id[&next_hop_ix], e);
                        break;
                    }

                    // do this after we send the event over
                    self.out_times[next_hop_ix] = tx_end;
                } // end EventType::packet
            } // end match
        } // end for loop
        println!("Router #{} done. {} pkts", self.id, self.count);
        return self.count;
    } // end start() function

} // end NIC methods

/*
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
*/
