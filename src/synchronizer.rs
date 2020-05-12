use std::collections::VecDeque;
use std::collections::HashMap;
//use ringbuf::RingBuffer;
use std::collections::BinaryHeap;
use std::cmp::Ordering;
use crossbeam::channel;

use crate::nic::*;

//                    s   ms  us  ns
const PRINT :u64 = 004_000_000_000;
//const PRINT :u64 = 000_001_000_000;
pub const DONE  :u64 = PRINT + 100_000;

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

pub struct EventReceiver {
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
