use std::thread;
use std::collections::VecDeque;
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
    NICRx {nic: usize, packet: Packet},
    NICEnable { nic: usize },
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
    threads : Vec<thread::JoinHandle<()>>,

    // outgoing events
    next_heap : RadixHeapMap<i64, Event>, // contains <=one event from each src
    missing_srcs : HashSet<usize>, // which srcs are missing

    event_q_out : HashMap<usize, ringbuf::Consumer<Event>>, // TODO split
    event_q_inc : HashMap<usize, ringbuf::Producer<Event>>, // TODO split

    // incoming events
    event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
    last_times : HashMap<usize, u64>, // last event from each queue
    safe_time: u64, // current safe time, for optimizing
}

impl EventReceiver {
    pub fn new(id : usize) -> EventReceiver {
        let (event_tx, event_rx) = mpsc::channel();

        EventReceiver {
            id,
            threads : Vec::new(),

            next_heap : RadixHeapMap::new(),
            missing_srcs : HashSet::new(),

            event_q_out : HashMap::new(),
            event_q_inc : HashMap::new(),

            event_rx,
            event_tx,
            last_times : HashMap::new(),
            safe_time : 0,
        }
    }

    pub fn connect(&mut self, other_id : usize) -> mpsc::Sender<Event> {
        // TODO if time > 0, panic

        // create event queue
        let q = RingBuffer::<Event>::new(10); // TODO look into effect of size on this...
        let (mut q_inc, mut q_out) = q.split();

        self.event_q_out.insert(other_id, q_out);
        self.event_q_inc.insert(other_id, q_inc);
        self.last_times.insert(other_id, 0); // initialize safe time to 0

        // let them know how to add events
        return self.event_tx.clone();
    }

    pub fn start(&mut self) -> thread::JoinHandle<()> {
        let handle = thread::spawn(move ||  {
            // mutates:
            //    - event_queues on the rx end
            //    - last_times
            //    - safe_time
            loop {
                // read in event from channel
                let event = self.event_rx.recv().unwrap();
                let event_time = event.time as u64;
                let event_src  = event.src;

                // enq in appropriate event queue
                self.event_q_inc.get_mut(&event.src).unwrap().push(event);

                // update safe proc time, needs to happen after append
                let old_safe_time = self.last_times[&event_src];
                self.last_times.insert(event_src, event_time);

                // remove and if need be, add to heap
                if old_safe_time == self.safe_time { // might need updating of safe_time
                    let (_, safe_time) = self.last_times.iter().min().unwrap(); // TODO check this gets min(value)
                    self.safe_time = *safe_time;

                    // TODO notify the sending loop
                    // safe_channel.send(safe_time)
                }
            }
        });


        return handle;
    }

    // TODO make into iterator?
    fn send_loop(&mut self) {
        // mutates:
        //    - event_queues on the tx end
        //    - next_heap
        //    - missing_srcs

        loop {
            // TODO receive from channel
            let safe_time = 1;

            // refill our heap with the missing sources
            for src in &self.missing_srcs {
                match self.event_q_out.get_mut(&src).unwrap().pop() {
                    None => continue,
                    Some(event) => self.next_heap.push(-event.time, event)
                }
            }

            // process events up till <= safe time
            while self.next_heap.peek_key().unwrap() <= safe_time {
                let (_, event) = self.next_heap.pop().unwrap();

                // TODO
                // if not empty_event:
                //     yield event

                // update heap
                match self.event_q_out.get_mut(&event.src).unwrap().pop() {
                    None => { self.missing_srcs.insert(event.src); break },
                    Some(event) => self.next_heap.push(-event.time, event)
                }
            }
        }
    }
}

pub struct NIC {
    // fundamental properties
    latency_ns: i64,
    ns_per_byte: i64,

    event_receiver : EventReceiver,
    id : u64,

    // stats
    pub count: u64,
}

impl NIC {
    pub fn new(id : u64, dst : u64) -> NIC {

        NIC {
            latency_ns: 10,
            ns_per_byte: 1,
            event_receiver : EventReceiver::new(id as usize),
            count : 0,
            id,
        }
    }


    pub fn start(&self) {
        // TODO start sending, receiving threads
    }
}

/*
impl Receiver for NIC {
    fn receive(&mut self, time: i64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, p: Packet) {
        //println!("Received packet #{}!", p.seq_num);

        self.queue.push_back(p);
        self.count += 1;

        // attempt send
        self.send(time, event_queue, false);
    }
}
*/

/*
type ServerID = usize;
#[derive(Debug)]
pub struct Server {
    server_id: ServerID
}

impl Server {
    pub fn new(server_id: ServerID) -> Server {
        Server { server_id }
    }
}

type ToRID = usize;
#[derive(Debug)]
pub struct ToR {
    // about me
    tor_id: ToRID,
    n_ports: usize,
    output_queues: Vec<NIC>,
}

impl ToR {
    pub fn new(tor_id : ToRID, n_ports: usize) -> ToR {
        ToR {
            tor_id,
            n_ports,
            output_queues: Vec::new(),
        }
    }
}

impl Receiver for ToR {
    fn receive(&mut self, time: i64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, packet: Packet) {
        let dst = packet.dst as usize;

        // TODO support inter-ToR traffic
        // put in the corresponding output queue
        self.output_queues[dst].receive(time, event_queue, packet);
    }
}
*/
