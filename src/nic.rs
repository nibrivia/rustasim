use std::collections::HashMap;
use ringbuf::*;
//use crossbeam::channel;

use crate::synchronizer::*;

#[derive(Debug)]
pub struct Packet {
    src: usize,
    dst: usize,
    seq_num: u64,
    size_byte: u64,

    ttl: u64,
    sent_ns: u64,
}

#[derive(Debug)]
pub struct Flow {
    pub src: usize,
    pub dst: usize,
    pub size_byte: u64,

    cwnd: u64,
    next_seq: u64,
}

const BYTES_PER_PACKET: u64 = 1500;

impl Flow {
    pub fn new(src : usize, dst : usize, n_packets : u64) -> Flow {
        Flow {
            src,
            dst,

            size_byte: n_packets*BYTES_PER_PACKET,
            cwnd: 1,
            next_seq: 0,
        }
    }
}

impl Iterator for Flow {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_seq*BYTES_PER_PACKET < self.size_byte {
            let p = Packet {
                src: self.src,
                dst: self.dst,
                seq_num: self.next_seq,
                size_byte: BYTES_PER_PACKET,
                ttl: 10,
                sent_ns: 0,
            };
            self.next_seq += 1;
            Some(p)
        } else {
            None
        }
    }
}


pub struct Router {
    id : usize,

    // fundamental properties
    latency_ns: u64,
    ns_per_byte: u64,

    id_to_ix : HashMap<usize, usize>,
    next_ix : usize,

    // event management
    event_receiver : EventReceiver,
    out_queues : Vec<Producer<Event>>,
    out_times  : Vec<u64>,

    // networking things
    route : Vec<usize>,

    // stats
    count: u64,
}

impl Router {
    pub fn new(id : usize) -> Router {

        Router {
            id,
            latency_ns: 10,
            ns_per_byte: 1,

            id_to_ix : HashMap::new(),
            next_ix : 0,

            event_receiver : EventReceiver::new(id),
            out_queues : Vec::new(),
            out_times  : Vec::new(),
            //out_notify : HashMap::new(),

            route : Vec::new(),

            count : 0,
        }
    }

    pub fn init_queue(&mut self, dst : usize, events : Vec<Event>) {
        let dst_ix = self.id_to_ix[&dst];
        for e in events {
            self.out_queues[dst_ix].push(e).unwrap();
        }
    }

    pub fn connect(&mut self, other : & mut Self) {
        let inc_channel = self.event_receiver.connect_incoming(other.id, self.next_ix);
        //let ret_channel = inc_channel.clone();

        self.id_to_ix.insert(other.id, self.next_ix);

        let tx_queue = other._connect(&self, inc_channel);
        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        //self.out_notify.insert(other.id, 0);

        //self.route.
        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        self.next_ix += 1;

        //return &mut inc_channel; // TODO hacky...
    }

    pub fn _connect(&mut self, other : &Self, tx_queue : Producer<Event>) -> Producer<Event> {
        self.id_to_ix.insert(other.id, self.next_ix);

        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        //self.out_notify.insert(other.id, 0);64

        // self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        let chan = self.event_receiver.connect_incoming(other.id, self.next_ix);

        self.next_ix += 1;
        return chan;
    }

    /*
    pub fn connect_incoming(&mut self, other_id: usize) -> mpsc::Sender<Event> {
    }

    pub fn connect_outgoing(&mut self, other_id : usize, tx_queue : mpsc::Sender<Event>) {
    }
    */

    // will never return
    pub fn start(mut self) -> u64 {
        //let event_channel = self.event_receiver.start();

        // kickstart stuff up
        //if self.id != 1 {
        for (dst_ix, out_q) in self.out_queues.iter_mut().enumerate() {
            out_q.push(Event{
                event_type: EventType::Update,
                src: self.id,
                time: self.latency_ns,
            }).unwrap();
            self.out_times[dst_ix] = self.latency_ns;
        }
        //}

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
                // This comes from below
                EventType::Missing(dsts) => {
                // We need the time from these friendos
                for dst_ix in dsts {
                    self.out_queues[dst_ix].push(Event{
                        event_type: EventType::Update,
                        src: self.id,
                        time: event.time + self.latency_ns,
                    }).unwrap();
                }
            },

            EventType::Update => {
                self.out_queues[event.src].push(Event{
                    event_type: EventType::Response,
                    src: self.id,
                    time: self.out_times[event.src] + self.latency_ns,
                }).unwrap();
            },

            EventType::Response => {},

            EventType::Packet(mut packet) => {
                //println!("\x1b[0;3{}m@{} Router {} received {:?} from {}\x1b[0;00m", self.id+1, event.time, self.id, packet, event.src);
                self.count += 1;
                if packet.dst == self.id {
                    // bounce!
                    packet.dst = packet.src;
                        packet.src = self.id;
                    }

                    // who
                    //let next_hop_ix = self.route[packet.dst];
                    let next_hop_ix = self.id_to_ix[&packet.dst];

                    // when
                    let cur_time = std::cmp::max(event.time, self.out_times[next_hop_ix]);
                    let tx_end = cur_time + self.ns_per_byte * packet.size_byte;
                    let rx_end = tx_end + self.latency_ns;

                    //println!("\x1b[0;3{}m@{} Router {} sent {:?} to {}@{}", self.id+1, event.time, self.id, packet, next_hop, rx_end);
                    // go
                    if let Err(_) = self.out_queues[next_hop_ix]
                        .push(Event{
                            event_type : EventType::Packet(packet),
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
