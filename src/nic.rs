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
    pub fn new() -> Flow {
        Flow {
            src: 0,
            dst: 0,

            size_byte: 200*BYTES_PER_PACKET,
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
        let q = RingBuffer::<Event>::new(10); // TODO look into effect of size on this...
        let (mut q_inc, mut q_out) = q.split(); // TODO remove mut?

        self.event_q_out.insert(other_id, q_out);
        self.event_q_inc.insert(other_id, q_inc);
        self.last_times.insert(other_id, 0); // initialize safe time to 0

        // let them know how to add events
        return self.event_tx.clone();
    }

    pub fn start(self) -> mpsc::Receiver<Event> {
        let mut event_q_inc = self.event_q_inc;
        let mut last_times  = self.last_times;
        let mut event_rx    = self.event_rx;

        let (safe_prod, safe_cons) = mpsc::channel();

        thread::spawn(move ||  {
            // mutates:
            //    - event_queues on the rx end
            //    - last_times
            //    - safe_time
            let safe_time : u64 = 0;
            loop {
                // read in event from channel
                let event = event_rx.recv().unwrap();
                let event_time = event.time as u64;
                let event_src  = event.src;

                // enq in appropriate event queue
                event_q_inc.get_mut(&event.src).unwrap().push(event).unwrap();

                // update safe proc time, needs to happen after append
                let old_safe_time = last_times[&event_src];
                last_times.insert(event_src, event_time).unwrap();

                // remove and if need be, add to heap
                if old_safe_time == safe_time { // might need updating of safe_time
                    let (_, safe_time) = last_times.iter().min().unwrap(); // TODO check this gets min(value)
                    // safe_time = *safe_time;

                    safe_prod.send(*safe_time).unwrap();
                }
            }
        });


        let mut event_q_out = self.event_q_out;
        let mut next_heap = self.next_heap;
        let mut missing_srcs = self.missing_srcs;

        let (event_prod, event_cons) = mpsc::channel();

        thread::spawn(move ||  {
            // mutates:
            //    - event_queues on the tx end
            //    - next_heap
            //    - missing_srcs

            loop {
                // Receive time from channel
                let safe_time : u64 = safe_cons.recv().unwrap();

                // refill our heap with the missing sources
                for src in &missing_srcs {
                    match event_q_out.get_mut(&src).unwrap().pop() {
                        None => continue,
                        Some(event) => next_heap.push(-event.time, event)
                    }
                }

                // process events up till <= safe time
                while -(next_heap.peek_key().unwrap()) <= safe_time as i64 {
                    let (_, event) = next_heap.pop().unwrap();
                    let src = event.src;

                    // if not empty_event:
                    //     yield event
                    event_prod.send(event).unwrap();

                    // update heap
                    match event_q_out.get_mut(&src).unwrap().pop() {
                        None => { missing_srcs.insert(src); break },
                        Some(new_event) => next_heap.push(-new_event.time, new_event)
                    }
                }
            }
        });

        return event_cons;
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

    // networking things
    route : HashMap<usize, usize>,

    // stats
    pub count: u64,
}

impl Router {
    pub fn new(id : usize) -> Router {

        Router {
            id,
            latency_ns: 10,
            ns_per_byte: 1,

            event_receiver : EventReceiver::new(id as usize),
            out_queues : HashMap::new(),

            route : HashMap::new(),

            count : 0,
        }
    }

    pub fn connect_incoming(&mut self, other_id: usize) -> mpsc::Sender<Event> {
        return self.event_receiver.connect_incoming(other_id);
    }

    pub fn connect_outgoing(&mut self, other_id : usize, tx_queue : mpsc::Sender<Event>) {
        self.out_queues.insert(other_id, tx_queue);
    }

    // will never return
    pub fn start(self) {
        let event_channel = self.event_receiver.start();

        loop {
            let event : Event = event_channel.recv().unwrap();
            match event.event_type {
                EventType::Empty => {}, // do nothing
                EventType::Packet(src, mut packet) => {
                    println!("Received {:?} from {}", packet, src);
                    if packet.dst == self.id {
                        // bounce!
                        packet.dst = packet.src;
                        packet.src = self.id;
                    }

                    let next_hop = self.route[&packet.dst];

                    let rx_delay = self.latency_ns + self.ns_per_byte * packet.size_byte;
                    self.out_queues[&next_hop]
                        .send(Event{
                            event_type : EventType::Packet(src, packet),
                            src: self.id,
                            time: event.time + rx_delay as i64,
                        })
                        .unwrap();
                } // boing...
            } // boing...
        } // boing...
    } // boing...
} // boing...
// splat
