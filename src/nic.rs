use std::collections::HashMap;
use std::collections::VecDeque;
//use ringbuf::RingBuffer;
use std::collections::BinaryHeap;
use std::cmp::Ordering;
use crossbeam::channel;

//                    s   ms  us  ns
const PRINT :u64 = 004_000_000_000;
//const PRINT :u64 = 000_001_000_000;
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

/*
pub trait Receiver {
    fn receive(&mut self, time: i64, event_queue: &mut BinaryHeap<Event>, packet: Packet);
}
*/

#[derive(Debug)]
pub enum EventType {
    Packet (Packet),
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

type NextEvent = (u64, usize);

struct EventReceiver {
    id : usize,
    id_to_ix : HashMap<usize, usize>,

    // outgoing events
    next_heap : BinaryHeap<NextEvent>, // contains <=one event from each src
    next_event_time : u64,
    missing_srcs : Vec<usize>, // which srcs are missing

    event_q : Vec<VecDeque<Event>>,

    // incoming events
    event_rx: channel::Receiver<Event>,
    event_tx: channel::Sender<Event>,
    last_times : Vec<u64>, // last event from each queue
    safe_time : u64, // last event from each queue

}

impl EventReceiver {
    pub fn new(id : usize) -> EventReceiver {
        let (event_tx, event_rx) = channel::bounded(512); // TODO think about size
        let next_heap = BinaryHeap::new();

        EventReceiver {
            id,
            id_to_ix : HashMap::new(),

            next_heap,
            next_event_time : 0,
            missing_srcs : Vec::new(),

            event_q : Vec::new(),

            event_rx,
            event_tx,
            last_times : Vec::new(),
            safe_time : 0,
        }
    }

    pub fn connect_incoming(&mut self, other_id : usize, other_ix : usize) -> channel::Sender<Event> {
        // create event queue
        let q = VecDeque::new();
        //q.reserve(256);

        self.id_to_ix.insert(other_id, other_ix);

        self.event_q.push(q);
        self.last_times.push(0); // initialize safe time to 0

        self.missing_srcs.push(other_ix);

        // let them know how to add events
        return self.event_tx.clone();
    }
}

impl Iterator for EventReceiver {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        // refill our heap with the missing sources
        let mut new_missing : Vec<usize> = Vec::new();
        for src in self.missing_srcs.iter() {
            // pop front of queue, if queue empty, keep in missing_srcs
            match self.event_q[*src].front() {
                None => { new_missing.push(*src); () },
                Some(event) => self.next_heap.push((DONE-event.time, *src)),
            }
        }
        self.missing_srcs = new_missing;

        // process events if we've heard form everyone and up till safe time
        if self.missing_srcs.is_empty() && DONE-self.next_heap.peek().unwrap().0 <= self.safe_time {
            let (_, src_ix) = self.next_heap.pop().unwrap();
            let event = self.event_q[src_ix].pop_front().unwrap();

            // update next event heap
            self.missing_srcs.push(src_ix);

            // return event
            return Some(event);
        }
        //println!("{} missing srcs {:?}", self.id, self.missing_srcs);



        // we have no more events to send... wait...
        loop {
            // read in event from channel, block if need be
            //println!("Router {} waiting for events...", self.id);
            let mut event = self.event_rx.recv().unwrap();

            // find out the ix we care about
            let event_src_ix = self.id_to_ix[&event.src];
            event.src = event_src_ix; // this ends up helping our parent too

            //println!("R{} received event {:?}", id, event);
            let event_time = event.time;

            // update safe proc time, needs to happen after append
            let old_last_time = self.last_times[event_src_ix];
            self.last_times[event_src_ix] = event_time;

            // update safe time if need be
            let old_safe_time = self.safe_time;
            if self.safe_time == old_last_time {
                self.safe_time = *self.last_times.iter().min().unwrap();
            }

            // enq in appropriate event queue
            self.event_q[event_src_ix].push_back(event);

            // safe time got updated, return the next event
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

    id_to_ix : HashMap<usize, usize>,
    next_ix : usize,

    // event management
    event_receiver : EventReceiver,
    out_queues : Vec<channel::Sender<Event>>,
    out_times  : Vec<u64>,

    // networking things
    route : HashMap<usize, usize>,

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

            route : HashMap::new(),

            count : 0,
        }
    }

    pub fn connect(&mut self, other : & mut Self) -> channel::Sender<Event> {
        let inc_channel = self.event_receiver.connect_incoming(other.id, self.next_ix);
        let ret_channel = inc_channel.clone();

        self.id_to_ix.insert(other.id, self.next_ix);

        let tx_queue = other._connect(&self, inc_channel);
        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        //self.out_notify.insert(other.id, 0);

        self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

        self.next_ix += 1;

        return ret_channel; // TODO hacky...
    }

    pub fn _connect(&mut self, other : &Self, tx_queue : channel::Sender<Event>) -> channel::Sender<Event> {
        self.id_to_ix.insert(other.id, self.next_ix);

        self.out_queues.push(tx_queue);
        self.out_times.push(0);
        //self.out_notify.insert(other.id, 0);

        self.route.insert(other.id, self.next_ix); // route to neighbour is neighbour

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
        for (dst_ix, out_q) in self.out_queues.iter().enumerate() {
            out_q.send(Event{
                event_type: EventType::Empty,
                src: self.id,
                time: self.latency_ns,
            }).unwrap();
            self.out_times[dst_ix] = self.latency_ns;
        }
        //}

        for dst in 0..11 {
            if !self.route.contains_key(&dst) {
                self.route.insert(dst, 0);
            }
        }

        println!("Router {} starting...", self.id);
        for event in self.event_receiver {
            //println!("@{} Router {} \x1b[0;3{}m got event {:?}!...\x1b[0;00m", event.time, self.id, self.id+2, event);
            if event.time > DONE {
                for c in &self.out_queues {
                    c.send(Event{
                        event_type: EventType::Empty,
                        src: self.id,
                        time: DONE+self.latency_ns,
                    }).unwrap();
                }
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
                    //let dst_ix = self.id_to_ix[&event.src];
                    let dst_ix = event.src;

                    // see if we need to send them anything
                    if event.time < self.out_times[dst_ix] {
                        continue
                    }

                    // update them
                    self.out_queues[dst_ix].send(Event{
                        event_type: EventType::Empty,
                        src: self.id,
                        time: event.time + self.latency_ns,
                    }).unwrap();

                    // update our notion of their time
                    self.out_times[dst_ix] = event.time;
                },

                EventType::Packet(mut packet) => {
                    //println!("\x1b[0;3{}m@{} Router {} received {:?} from {}\x1b[0;00m", self.id+1, event.time, self.id, packet, event.src);
                    self.count += 1;
                    if packet.dst == self.id {
                        // bounce!
                        packet.dst = packet.src;
                        packet.src = self.id;
                    }

                    // who
                    let next_hop_ix = self.route[&packet.dst];

                    // when
                    let cur_time = std::cmp::max(event.time, self.out_times[next_hop_ix]);
                    let tx_end = cur_time + self.ns_per_byte * packet.size_byte;
                    let rx_end = tx_end + self.latency_ns;

                    //println!("\x1b[0;3{}m@{} Router {} sent {:?} to {}@{}", self.id+1, event.time, self.id, packet, next_hop, rx_end);
                    // go
                    if let Err(_) = self.out_queues[next_hop_ix]
                        .send(Event{
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
