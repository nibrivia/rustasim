use std::fmt;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
//use std::collections::HashMap;
use crossbeam::queue::spsc;
//use ringbuf::*;

use crate::tcp::*;


#[derive(Debug)]
pub enum EventType {
    Flow(Flow),
    Packet(Packet),
    Missing(Vec<usize>),
    Update,
    Response,
    Close,
    //NICEnable { nic: usize },
}

#[derive(Debug)]
pub struct Event {
    pub time: u64,
    pub src: usize,
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




pub struct EventScheduler {
    id: usize,

    // outgoing events
    next_heap: BinaryHeap<Event>, // contains <=one event from each src
    missing_srcs: Vec<usize>,     // which srcs are missing

    event_q: Vec<spsc::Consumer<Event>>,

    safe_time: u64, // last event from each queue
}

impl fmt::Display for EventScheduler {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EventScheduler for {}", self.id)
    }
}

impl EventScheduler {
    pub fn new(id: usize) -> EventScheduler {
        let next_heap = BinaryHeap::new();

        EventScheduler {
            id,
            //id_to_ix: HashMap::new(),

            next_heap,
            missing_srcs: Vec::new(),

            event_q: Vec::new(),

            safe_time: 0,
        }
    }

    pub fn connect_incoming(&mut self, other_ix: usize) -> spsc::Producer<Event> {
        // create event queue
        let (prod, cons) = spsc::new(128);
        //let (prod, cons) = q.split();

        self.event_q.push(cons);

        self.missing_srcs.push(other_ix);

        return prod;
    }
}

impl Iterator for EventScheduler {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        // copy self state to local, will update on exit
        let safe_time = self.safe_time;

        loop {
            // process events if we've heard form everyone or up till safe time
            if self.missing_srcs.is_empty()
                || (!self.next_heap.is_empty() && self.next_heap.peek().unwrap().time <= safe_time)
            {
                // get the next event
                let event = self.next_heap.pop().unwrap();
                //let event_src_ix = event.src;

                // attempt to refill what we just emptied
                match self.event_q[event.src].pop() {
                    Err(_) => {
                        self.missing_srcs.push(event.src);
                    }
                    Ok(mut new_event) => {
                        new_event.src = event.src; // update event src now
                        self.next_heap.push(new_event)
                    }
                }

                // update safe time : this is used if we have multiple event from different sources
                // we could process, so even if we are missing sources, we can still send those
                //self.safe_time = event.time;

                // done!
                return Some(event);
            }
            //println!("{} missing srcs {:?}", self.id, self.missing_srcs);

            // TODO implement "Update" mechanism

            // refill our heap with the missing sources
            let mut new_missing: Vec<usize> = Vec::new();
            for src in self.missing_srcs.iter() {
                // pop front of queue, if queue empty, keep in missing_srcs
                match self.event_q[*src].pop() {
                    Err(_) => {
                        new_missing.push(*src);
                    }
                    Ok(mut event) => {
                        event.src = *src; // update event src now
                        self.next_heap.push(event)
                    }
                }
            }

            self.missing_srcs = new_missing;

            // If we're still waiting on sources -> deadlock, trigger update
            // TODO avoid repeated send
            if !self.missing_srcs.is_empty() {
                let event = Event {
                    event_type : EventType::Missing(self.missing_srcs.to_vec()),
                    src: 0, // doesn't matter
                    time: self.safe_time,
                };

                return Some(event);
            }

        }
    }
}
