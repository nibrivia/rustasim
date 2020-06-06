use std::fmt;
//use std::time;
//use std::thread;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use crossbeam::queue::spsc;
//use ringbuf::*;

use crate::tcp::*;


#[derive(Debug)]
pub enum EventType {
    Flow(Flow),
    Packet(Packet),
    Stalled,
    Null,
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
    ix_to_id: HashMap<usize, usize>,

    state: SchedulerState,

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

enum SchedulerState {
    Going,
    Stalled,
    Waiting,
}

impl EventScheduler {
    pub fn new(id: usize) -> EventScheduler {
        let next_heap = BinaryHeap::new();

        EventScheduler {
            id,
            //id_to_ix: HashMap::new(),
            ix_to_id: HashMap::new(),

            state: SchedulerState::Stalled,

            next_heap,
            missing_srcs: Vec::new(),

            event_q: Vec::new(),

            safe_time: 0,
        }
    }

    pub fn connect_incoming(&mut self, other_ix: usize, other_id: usize) -> spsc::Producer<Event> {
        // create event queue
        let (prod, cons) = spsc::new(128);
        //let (prod, cons) = q.split();
        self.ix_to_id.insert(other_ix, other_id);

        self.event_q.push(cons);

        self.missing_srcs.push(other_ix);

        return prod;
    }
}

impl Iterator for EventScheduler {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.state {
                // process events if we've heard form everyone or up till safe time
                SchedulerState::Going => {
                    let mut go = self.missing_srcs.is_empty();
                    while go
                        || (!self.next_heap.is_empty() && self.next_heap.peek().unwrap().time <= self.safe_time)
                    {
                        // get the next event
                        let event = self.next_heap.pop().unwrap();
                        self.safe_time = event.time;
                        //let event_src_ix = event.src;

                        // attempt to refill what we just emptied
                        match self.event_q[event.src].pop() {
                            Err(_) => {
                                self.missing_srcs.push(event.src);
                                go = false; // we're done :/
                            }
                            Ok(mut new_event) => {
                                new_event.src = event.src; // update event src now
                                self.next_heap.push(new_event)
                            }
                        }

                        // Null events are ignored by the user
                        if let EventType::Null = event.event_type {
                            continue;
                        }

                        // done!
                        return Some(event);
                    }

                    self.state = SchedulerState::Stalled;
                },

                // We're still waiting on sources -> may be deadlock, trigger update
                SchedulerState::Stalled => {
                    let event = Event {
                        event_type : EventType::Stalled,
                        src: 0, // doesn't matter
                        time: self.safe_time,
                    };

                    self.state = SchedulerState::Waiting;
                    return Some(event);
                },

                // block here until we can make progress
                SchedulerState::Waiting => {
                    for src in self.missing_srcs.iter() {
                        // TODO non-spin block?
                        //println!("@{} #{} waiting on #{}",
                        //    self.safe_time, self.id, self.ix_to_id[src]);
                        while self.event_q[*src].len() == 0 {
                        }

                        let mut new_event = self.event_q[*src].pop().unwrap();

                        new_event.src = *src; // update event src id->ix now
                        // TODO investigate if immediate return is helpful
                        self.next_heap.push(new_event);
                    }

                    // good to go :)
                    self.missing_srcs.clear();
                    self.state = SchedulerState::Going;
                },

            } // end state match
        } // end loop
    } // end next()
} // end Iterator impl
