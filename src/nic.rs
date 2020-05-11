//use std::thread;
//use std::collections::VecDeque;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
//use ringbuf::RingBuffer;
use std::collections::BinaryHeap; //TODO might be better than RadixHeapMap?
use std::cmp::Ordering;
//use radix_heap::RadixHeapMap;
use std::sync::mpsc;
// use crate::scheduler;

// pub type SizeByte = u64;
//
//                    s   ms  us  ns
const PRINT :u64 = 002_000_000_000;
const DONE  :u64 = PRINT + 100_000;

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
    src: usize,
    dst: usize,

    size_byte: u64,
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

/*
pub trait Receiver {
    fn receive(&mut self, time: i64, event_queue: &mut BinaryHeap<Event>, packet: Packet);
}
*/

#[derive(Debug)]
pub enum EventType {
    Packet (usize, Packet),
    Empty,
    //Close,
    //NICEnable { nic: usize },
}

#[derive(Debug)]
pub struct Event {
    pub time: u64,
    pub src : usize,
    pub event_type: EventType,
    //function: Box<dyn FnOnce() -> ()>,
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        other.time.cmp(&self.time)
    }
}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))

    }
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl Eq for Event {} // don't use function

struct EventReceiver {
    id : usize,

    // outgoing events
    next_heap : BinaryHeap<Event>, // contains <=one event from each src
    missing_srcs : HashSet<usize>, // which srcs are missing

    event_q : HashMap<usize, VecDeque<Event>>,
    //event_q_out : HashMap<usize, ringbuf::Consumer<Event>>,
    //event_q_inc : HashMap<usize, ringbuf::Producer<Event>>,

    // incoming events
    event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
    last_times : HashMap<usize, u64>, // last event from each queue
    safe_time : u64, // last event from each queue
}

impl EventReceiver {
    pub fn new(id : usize) -> EventReceiver {
        let (event_tx, event_rx) = mpsc::channel();
        let next_heap = BinaryHeap::new();

        EventReceiver {
            id,

            next_heap,
            missing_srcs : HashSet::new(),

            event_q : HashMap::new(), // TODO slower, but doesn't have out of bounds issues...
            //event_q_out : HashMap::new(),
            //event_q_inc : HashMap::new(),

            event_rx,
            event_tx,
            last_times : HashMap::new(),
            safe_time : 0,
        }
    }

    pub fn connect_incoming(&mut self, other_id : usize) -> mpsc::Sender<Event> {
        // create event queue
        let mut q = VecDeque::new(); // TODO look into effect of size on this...
        q.reserve(512);

        self.event_q.insert(other_id, q);
        self.last_times.insert(other_id, 0); // initialize safe time to 0

        self.missing_srcs.insert(other_id);

        // let them know how to add events
        return self.event_tx.clone();
    }
}

impl Iterator for EventReceiver {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        // refill our heap with the missing sources
        let mut new_missing : HashSet<usize> = HashSet::new();
        for src in (&mut self.missing_srcs).iter() {

            // pop front of queue, if queue empty, keep in missing_srcs
            match self.event_q.get_mut(&src).unwrap().pop_front() {
                None => { new_missing.insert(*src); () },
                Some(event) => self.next_heap.push(event),
            }
        }
        self.missing_srcs = new_missing;

        // process events if we've heard form everyone and up till safe time
        if self.missing_srcs.len() == 0 && self.next_heap.peek().unwrap().time <= self.safe_time {
            let event = self.next_heap.pop().unwrap();

            // update next event heap
            self.missing_srcs.insert(event.src);

            // return event
            return Some(event);
        }

        // we have no events... wait...
        loop {
            // read in event from channel, block if need be
            //println!("Router {} waiting for events...", self.id);
            let event = self.event_rx.recv().unwrap();
            //println!("R{} received event {:?}", id, event);

            let event_time = event.time;
            let event_src  = event.src;

            //println!("R{} received event ok", id);

            // update safe proc time, needs to happen after append
            let old_safe_time = self.safe_time;
            self.last_times.insert(event_src, event_time).unwrap();
            self.safe_time = *self.last_times.values().min().unwrap();

            // enq in appropriate event queue
            self.event_q.get_mut(&event_src).unwrap().push_back(event);

            //println!("S{} received safe_time of {}", id, safe_time);
            if old_safe_time < self.safe_time {
                return self.next();
            }
        }
    }
}

pub struct Router {
    id : usize,

    // fundamental properties
    latency_ns: u64,
    ns_per_byte: u64,

    // event management
    event_receiver : EventReceiver,
    out_queues : HashMap<usize, mpsc::Sender<Event>>,
    out_times  : HashMap<usize, u64>,

    // networking things
    route : HashMap<usize, usize>,

    // stats
    count: u64,
}

impl Router {
    pub fn new(id : usize) -> Router {

        Router {
            id,
            latency_ns: 100,
            ns_per_byte: 1,

            event_receiver : EventReceiver::new(id),
            out_queues : HashMap::new(),
            out_times  : HashMap::new(),
            //out_notify : HashMap::new(),

            route : HashMap::new(),

            count : 0,
        }
    }

    pub fn connect(&mut self, other : & mut Self) -> mpsc::Sender<Event> {
        let inc_channel = self.event_receiver.connect_incoming(other.id);
        let ret_channel = inc_channel.clone();

        let tx_queue = other._connect(&self, inc_channel);
        self.out_queues.insert(other.id, tx_queue);
        self.out_times.insert(other.id, 0);
        //self.out_notify.insert(other.id, 0);

        self.route.insert(other.id, other.id); // route to neighbour is neighbour

        return ret_channel; // TODO hacky...
    }

    pub fn _connect(&mut self, other : &Self, tx_queue : mpsc::Sender<Event>) -> mpsc::Sender<Event> {
        self.out_queues.insert(other.id, tx_queue);
        self.out_times.insert(other.id, 0);
        //self.out_notify.insert(other.id, 0);

        self.route.insert(other.id, other.id); // route to neighbour is neighbour

        return self.event_receiver.connect_incoming(other.id);
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
        for (dst, out_q) in &self.out_queues {
            out_q.send(Event{
                event_type: EventType::Empty,
                src: self.id,
                time: self.latency_ns,
            }).unwrap();
            self.out_times.insert(*dst, self.latency_ns);
        }
        //}

        // TODO actual routing
        if self.id == 0 {
            self.route.insert(2, 1);
        }
        if self.id == 2 {
            self.route.insert(0, 1);
        }

        println!("Router {} starting...", self.id);
        for event in self.event_receiver {
            //println!("@{} Router {} \x1b[0;3{}m got event {:?}!...\x1b[0;00m", event.time, self.id, self.id+2, event);
            if event.time > DONE {
                break;
            }
            match event.event_type {
                /*
                EventType::Close => {
                    println!("{}", self.count);
                    return self.count;
                },*/

                EventType::Empty => {
                    // whoever sent us this needs to know how far they can advance
                    let dst = event.src;

                    // see if we need to send them anything
                    if event.time < self.out_times[&dst] {
                        continue
                    }

                    // update them
                    self.out_queues[&dst].send(Event{
                        event_type: EventType::Empty,
                        src: self.id,
                        time: event.time + self.latency_ns,
                    }).unwrap();

                    // update our notion of their time
                    self.out_times.insert(dst, event.time);
                },

                EventType::Packet(src, mut packet) => {
                    //println!("@{} Router {} received {:?} from {}", event.time, self.id, packet, src);
                    self.count += 1;
                    if packet.dst == self.id {
                        // bounce!
                        packet.dst = packet.src;
                        packet.src = self.id;
                        //continue
                    }

                    let next_hop = self.route[&packet.dst];

                    // sender
                    let cur_time = std::cmp::max(event.time, self.out_times[&next_hop]);
                    let tx_end = cur_time + self.ns_per_byte * packet.size_byte;

                    // receiver
                    let rx_end = tx_end + self.latency_ns;
                    //println!("@{} Router {} sent {:?} to {}", event.time, self.id, packet, next_hop);
                    self.out_queues[&next_hop]
                        .send(Event{
                            event_type : EventType::Packet(src, packet),
                            src: self.id,
                            time: rx_end,
                        })
                        .unwrap();

                    // do this after we send the event over
                    self.out_times.insert(next_hop, tx_end);
                } // boing...
            } // boing...
        } // boing...
        return self.count;
    } // boing...
} // boing...
// splat
