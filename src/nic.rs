use std::thread;
//use std::collections::VecDeque;
use std::collections::HashMap;
use std::collections::HashSet;
use ringbuf::RingBuffer;
// use std::collections::BinaryHeap; TODO might be better than RadixHeapMap?
use radix_heap::RadixHeapMap;
use std::sync::mpsc;
// use crate::scheduler;

// pub type SizeByte = u64;
//
const PRINT :i64 = 100_000_000;
const DONE  :i64 = PRINT + 100_000;

#[derive(Debug)]
pub struct Packet {
    src: usize,
    dst: usize,
    seq_num: u64,
    size_byte: u64,

    ttl: u64,
    sent_ns: i64,
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

pub trait Receiver {
    fn receive(&mut self, time: i64, event_queue: &mut RadixHeapMap<i64, Event>, packet: Packet);
}

#[derive(Debug)]
pub enum EventType {
    Packet (usize, Packet),
    Empty,
    //Close,
    //NICEnable { nic: usize },
}

#[derive(Debug)]
pub struct Event {
    pub time: i64,
    pub src : usize,
    pub event_type: EventType,
    //function: Box<dyn FnOnce() -> ()>,
}

struct EventReceiver {
    id : usize,

    // outgoing events
    next_heap : RadixHeapMap<i64, Event>, // contains <=one event from each src
    missing_srcs : HashSet<usize>, // which srcs are missing

    event_q_out : HashMap<usize, ringbuf::Consumer<Event>>,
    event_q_inc : HashMap<usize, ringbuf::Producer<Event>>,

    // incoming events
    event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
    last_times : HashMap<usize, u64>, // last event from each queue
}

impl EventReceiver {
    pub fn new(id : usize) -> EventReceiver {
        let (event_tx, event_rx) = mpsc::channel();

        EventReceiver {
            id,

            next_heap : RadixHeapMap::new(),
            missing_srcs : HashSet::new(),

            event_q_out : HashMap::new(),
            event_q_inc : HashMap::new(),

            event_rx,
            event_tx,
            last_times : HashMap::new(),
        }
    }

    pub fn connect_incoming(&mut self, other_id : usize) -> mpsc::Sender<Event> {
        // create event queue
        let q = RingBuffer::<Event>::new(25600); // TODO look into effect of size on this...
        let (q_inc, q_out) = q.split(); // TODO remove mut?

        self.event_q_out.insert(other_id, q_out);
        self.event_q_inc.insert(other_id, q_inc);
        self.last_times.insert(other_id, 0); // initialize safe time to 0

        self.missing_srcs.insert(other_id);

        // let them know how to add events
        return self.event_tx.clone();
    }

    pub fn start(self) -> mpsc::Receiver<Event> {
        let mut event_q_inc = self.event_q_inc;
        let mut last_times  = self.last_times;
        let     event_rx    = self.event_rx;
        let id = self.id;

        let (safe_prod, safe_cons) = mpsc::channel();

        thread::spawn(move ||  {
            // mutates:
            //    - event_queues on the rx end
            //    - last_times
            //    - safe_time
            let mut safe_time : u64 = 0;
            safe_prod.send(0).unwrap();
            loop {
                // read in event from channel
                //println!("R{} waiting for events...", id);
                let event = event_rx.recv();
                let event = match event {
                    Ok(event) => event,
                    Err(error) => panic!("R{} received event error <{:?}>", id, error),
                };
                //println!("R{} received event {:?}", id, event);
                let event_time = event.time as u64;
                let event_src  = event.src;

                // enq in appropriate event queue
                event_q_inc.get_mut(&event.src).unwrap().push(event).unwrap();
                //println!("R{} received event ok", id);

                // update safe proc time, needs to happen after append
                let old_safe_time = last_times[&event_src];
                last_times.insert(event_src, event_time).unwrap();

                // remove and if need be, add to heap
                if old_safe_time == safe_time { // might need updating of safe_time
                    safe_time = *last_times.values().min().unwrap(); // TODO check this gets min(value)
                }

                match safe_prod.send(safe_time) {
                    Ok(_) => (),
                    Err(error) => panic!("R{} send safetime error <{:?}>", id, error),
                };

                if safe_time > DONE as u64 { return; }
            }
        });


        let mut event_q_out = self.event_q_out;
        let mut next_heap = self.next_heap;
        let mut missing_srcs = self.missing_srcs;
        let id = self.id;

        let (event_prod, event_cons) = mpsc::channel();

        thread::spawn(move ||  {
            // mutates:
            //    - event_queues on the tx end
            //    - next_heap
            //    - missing_srcs

            loop {
                // Receive time from channel
                //println!("S{} waiting for safe time...", id);
                let safe_time = safe_cons.recv();
                let safe_time = match safe_time {
                    Ok(event) => event,
                    Err(error) => panic!("S{} received error {:?}", id, error),
                };
                //println!("S{} received safe_time of {}", id, safe_time);

                // refill our heap with the missing sources
                let mut new_missing : HashSet<usize> = HashSet::new();
                for src in (&mut missing_srcs).iter() {
                    match event_q_out.get_mut(&src).unwrap().pop() {
                        None => { new_missing.insert(*src); () },
                        Some(event) => next_heap.push(-event.time, event),
                    }
                }

                missing_srcs = new_missing;

                /*
                for src in added_srcs.iter() {
                    missing_srcs.remove(&src);
                }
                */

                if missing_srcs.len() > 0 {
                    //println!("S{} missing sources...", id);
                    continue // keep waiting
                }

                // process events up till <= safe time
                while -(next_heap.peek_key().unwrap()) <= safe_time as i64 {
                    let (_, event) = next_heap.pop().unwrap();
                    let src = event.src;

                    // if not empty_event:
                    //     yield event
                    //println!("S{} sending event {:?}...", id, event);
                    match event_prod.send(event) {
                        Ok(_) => (),
                        Err(error) => panic!("S{} send event error {:?}", id, error),
                    };

                    // update heap
                    match event_q_out.get_mut(&src).unwrap().pop() {
                        None => { missing_srcs.insert(src); break },
                        Some(new_event) => next_heap.push(-new_event.time, new_event)
                    }
                }

                if safe_time > DONE as u64 { return; }
            }
        });

        return event_cons;
    }
}

pub struct Router {
    id : usize,

    // fundamental properties
    latency_ns: i64,
    ns_per_byte: i64,

    // event management
    event_receiver : EventReceiver,
    out_queues : HashMap<usize, mpsc::Sender<Event>>,
    out_times  : HashMap<usize, i64>,
    out_notify : HashMap<usize, i64>,

    // networking things
    route : HashMap<usize, usize>,

    // stats
    pub count: u64,
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
            out_notify : HashMap::new(),

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
        self.out_notify.insert(other.id, 0);

        self.route.insert(other.id, other.id); // route to neighbour is neighbour

        return ret_channel; // TODO hacky...
    }

    pub fn _connect(&mut self, other : &Self, tx_queue : mpsc::Sender<Event>) -> mpsc::Sender<Event> {
        self.out_queues.insert(other.id, tx_queue);
        self.out_times.insert(other.id, 0);
        self.out_notify.insert(other.id, 0);

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
    pub fn start(mut self) {
        let event_channel = self.event_receiver.start();

        // kickstart stuff up
        if self.id != 1 {
            for (dst, out_q) in &self.out_queues {
                out_q.send(Event{
                    event_type: EventType::Empty,
                    src: self.id,
                    time: self.latency_ns,
                }).unwrap();
                self.out_times.insert(*dst, self.latency_ns);
            }
        }

        if self.id == 0 {
            self.route.insert(2, 1);
        }
        if self.id == 2 {
            self.route.insert(0, 1);
        }

        let mut print = true;
        loop {
            //println!("Router {} waiting...", self.id);
            let event = event_channel.recv();
            let event = match event {
                Ok(event) => event,
                Err(error) => panic!("Router {} received error {:?}", self.id, error),
            };
            //println!("Router {} \x1b[0;3{}m got event {:?}!...\x1b[0;00m", self.id, self.id+2, event);
            if event.time > PRINT && print {
                println!("{}", self.count);
                print = false;
            }
            if event.time > DONE {
                return;
            }
            match event.event_type {
                EventType::Empty => {
                    let dst = event.src;
                    let cur_time = std::cmp::max(event.time, self.out_times[&dst]);
                    self.out_times.insert(dst, cur_time);
                    if cur_time+self.latency_ns < self.out_notify[&dst] {
                        continue
                    }
                    self.out_notify.insert(dst, cur_time+self.latency_ns);
                    self.out_queues[&dst].send(Event{
                        event_type: EventType::Empty,
                        src: self.id,
                        time: cur_time + self.latency_ns,
                    }).unwrap();
                }, // do nothing
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
                    let tx_end = cur_time + self.ns_per_byte * packet.size_byte as i64;
                    self.out_times.insert(next_hop, tx_end);

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
                } // boing...
            } // boing...
        } // boing...
    } // boing...
} // boing...
// splat
