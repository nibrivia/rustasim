use std::collections::VecDeque;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::BinaryHeap;
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

#[derive(Debug)]
struct EventReceiver {
    id : usize,

    // outgoing events
    next_heap : RadixHeapMap<i64, Event>, // contains <=one event from each src
    missing_srcs : HashSet<usize>, // which srcs are missing

    event_queues: HashMap<usize, VecDeque<Event>>, // split

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

            next_heap : RadixHeapMap::new(),
            missing_srcs : HashSet::new(),

            event_queues : HashMap::new(),

            event_rx,
            event_tx,
            last_times : HashMap::new(),
            safe_time : 0,
        }
    }

    pub fn connect(&mut self, other_id : usize) -> mpsc::Sender<Event> {
        // TODO if time > 0, panic

        // create event queue
        self.event_queues.insert(other_id, VecDeque::new());

        // let them know how to add events
        return self.event_tx.clone();
    }

    pub fn recv_loop(&mut self) {
        loop {
            // read in event from channel
            let event = self.event_rx.recv().unwrap();
            let event_time = event.time as u64;
            let event_src  = event.src;

            // enq in appropriate event queue
            self.event_queues.get_mut(&event.src).unwrap().push_back(event);


            // update safe proc time, needs to happen after append
            let old_safe_time = self.last_times[&event_src];
            self.last_times.insert(event_src, event_time);

            // remove and if need be, add to heap
            if old_safe_time == self.safe_time { // might need updating of safe_time
                let (_, safe_time) = self.last_times.iter().min().unwrap();
                self.safe_time = *safe_time;

                // TODO notify the sending loop
            }

        }
    }

    // TODO make iterator?
    pub fn send_loop(&mut self) {
        loop {
            // wait on condition variable? ...

            // current safe time?
            // keep waiting if we got no-one... TODO might not be needed?
            if self.last_times.len() == 0 { continue } 

            let safe_time = self.last_times.iter().min().unwrap();
            // process events up till <= safe time



            // send new events
            // self.event_rx.send().unwrap();

            // if no events, wait until "wakeup"
        }
    }

    // iter()
}

#[derive(Debug)]
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



    /*
    pub fn send(&mut self, time: i64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, enable: bool) {
        self.enabled = self.enabled | enable;

        if !self.enabled || self.queue.len() == 0 {
            return
        }

        let packet = self.queue.pop_front().unwrap();
        self.enabled = false;

        let tx_delay = self.ns_per_byte * packet.size_byte as i64;
        let rx_delay = self.latency_ns + tx_delay;

        event_queue.push(-(time + tx_delay), scheduler::Event{time: time+tx_delay, event_type: scheduler::EventType::NICEnable{nic: 0}});
        event_queue.push(-(time + rx_delay), scheduler::Event{time: time+rx_delay, event_type: scheduler::EventType::NICRx{nic: 0, packet}});
    }
    */
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
