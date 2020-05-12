//use std::collections::VecDeque;
use std::collections::HashMap;
use std::collections::BinaryHeap;
use std::cmp::Ordering;
use std::thread;
//use ringbuf::Ringbuffer;
use ringbuf::*;
use core::time::Duration;
use crossbeam::channel;

use crate::nic::*;

//                    s   ms  us  ns
const PRINT :u64 = 001_000_000_000;
//const PRINT :u64 = 000_001_000_000;
pub const DONE  :u64 = PRINT + 100_000;
//const OFFSET    :u64 = DONE  + 100_000;

#[derive(Debug)]
pub enum EventType {
    Packet (Packet),
    Missing (Vec<usize>),
    Update,
    Response,
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

pub struct EventReceiver {
    id : usize,
    id_to_ix : HashMap<usize, usize>,

    // outgoing events
    next_heap : BinaryHeap<Event>, // contains <=one event from each src
    missing_srcs : Vec<usize>, // which srcs are missing

    event_q : Vec<Consumer<Event>>,

    // incoming events
    //event_rx: channel::Receiver<Event>,
    //event_tx: channel::Sender<Event>,
    //last_missing: u64,
    last_times : Vec<u64>, // last event from each queue
    safe_time : u64, // last event from each queue

}

impl EventReceiver {
    pub fn new(id : usize) -> EventReceiver {
        //let (event_tx, event_rx) = channel::bounded(1024); // TODO think about size
        let next_heap = BinaryHeap::new();

        EventReceiver {
            id,
            id_to_ix : HashMap::new(),

            next_heap,
            missing_srcs : Vec::new(),

            event_q : Vec::new(),

            //event_rx,
            //event_tx,
            last_times : Vec::new(),
            safe_time : 0,
            //last_missing : 0,
        }
    }

    pub fn connect_incoming(&mut self, other_id : usize, other_ix : usize) -> Producer<Event> {
        // create event queue
        //let q = VecDeque::new();
        let q = RingBuffer::new(1024);
        let (prod, cons) = q.split();
        //q.reserve(256);

        self.id_to_ix.insert(other_id, other_ix);

        self.event_q.push(cons);
        self.last_times.push(0); // initialize safe time to 0

        self.missing_srcs.push(other_ix);

        // let them know how to add events
        //return self.event_tx.clone();
        return prod;
    }
    
    pub fn start(self, channel : channel::Sender<Event>) {
        thread::spawn(move || {
            for event in self {
                channel.send(event).unwrap();
            }
        });
    }
}

impl Iterator for EventReceiver {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        // copy self state to local, will update on exit
        let safe_time = self.safe_time;
        //let mut wait = true;

        loop {
            // process events if we've heard form everyone and up till safe time
            if self.missing_srcs.is_empty() ||
                (!self.next_heap.is_empty() && self.next_heap.peek().unwrap().time <= safe_time) {
            //if self.missing_srcs.is_empty() {
                //println!("{} heap size {}, missing_srcs {:?}", self.id, self.next_heap.len(), self.missing_srcs);
                let mut event = self.next_heap.pop().unwrap();
                let event_src_ix = self.id_to_ix[&event.src];
                event.src = event_src_ix;

                // refill what we just emptied
                match self.event_q[event_src_ix].pop() {
                    None => { self.missing_srcs.push(event.src); () },
                    Some(event) => self.next_heap.push(event),
                }

                // return event
                self.safe_time = event.time;
                return Some(event);
            }
            //println!("{} missing srcs {:?}", self.id, self.missing_srcs);

            if safe_time >= DONE {
                return None;
            }

            // keep trying until safe_time updates
            //let old_safe_time = safe_time;
            //while old_safe_time == safe_time {
            /*
            loop {
                // read in event from channel, block if need be
                // TODO use try_recv, if empty, poke above for sending an Empty
                let event = self.event_rx.recv_timeout(Duration::from_micros(40));
                let mut event = match event {
                    Ok(event) => event,
                    Err(_err) => {
                        // first time: go to filling up missing srcs
                        // second time: ask for update
                        /*
                        if wait {
                            wait = false;
                            break;
                        }*/

                        // timeout: ask for update
                        return Some(Event{
                            time: safe_time,
                            src: 0,
                            event_type : EventType::Missing(self.missing_srcs.to_vec()),
                            });
                    }
                };

                //let event_time = event.time;
                let event_src  = event.src;

                // find out the ix we care about
                let event_src_ix = self.id_to_ix[&event_src];
                event.src = event_src_ix; // this ends up helping our parent too

                // enq in appropriate event queue

                if let EventType::Update = event.event_type {
                    return Some(event); // they need a response immediately
                } else {
                    let exit = self.event_q[event_src_ix].is_empty();
                    self.event_q[event_src_ix].push_back(event);
                    if exit {
                        break;
                    }
                }

                // update safe proc time
                //let old_last_time = self.last_times[event_src_ix];
                //self.last_times[event_src_ix] = event_time;

                // update safe time if need be
                //if safe_time == old_last_time {
                    //break;
                //}
            }
                */

            // refill our heap with the missing sources
            let mut new_missing : Vec<usize> = Vec::new();
            for src in self.missing_srcs.iter() {
                // pop front of queue, if queue empty, keep in missing_srcs
                match self.event_q[*src].pop() {
                    None => { new_missing.push(*src); () },
                    Some(event) => self.next_heap.push(event),
                }
            }
            self.missing_srcs = new_missing;

        }
    }
}
