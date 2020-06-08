use std::fmt;
use std::mem;
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


#[derive(Debug)]
struct Merger {
    // the input queues
    in_queues : Vec<spsc::Consumer<Event>>,
    n_layers : usize,

    // the queue to pull from
    winner_q : usize,

    // the loser queue
    loser_q : Vec<usize>,
    loser_e : Vec<Event>,
}

fn ltr_walk(n_nodes: usize) -> Vec<usize> {
    let n_layers = (n_nodes as f32).log2().ceil() as usize;

    // visited structure
    let mut visited : Vec<bool> = Vec::new();
    for _ in 0..n_nodes+1 {
        visited.push(false);
    }

    let mut cur_index = 2_usize.pow((n_layers-1) as u32);

    let mut indices = Vec::new();

    indices.push(cur_index);
    visited[cur_index] = true;
    let mut going_up = true;


    for _ in 0..n_nodes-1 {
        if going_up {
            while visited[cur_index] {
                cur_index /= 2;
            }
        } else {
            if cur_index * 2 + 1 >= n_nodes {
                going_up = true;
                continue;
            }

            cur_index = cur_index * 2 + 1;
            while cur_index * 2 < n_nodes {
                cur_index *= 2;
            }
        }

        indices.push(cur_index);
        visited[cur_index] = true;

        going_up = !going_up;
    }

    // hacky but w/e
    if visited[0] {
        indices.pop();
    }

    return indices;
}

impl Merger {
    // Takes all the queues
    fn new(in_queues : Vec<spsc::Consumer<Event>>) -> Merger {

        let mut loser_q = Vec::new();
        let mut loser_e = Vec::new();
        let winner_q = 0;

        // index 0 never gets used
        loser_q.push(0);
        loser_e.push(Event { time : 0, event_type: EventType::Null, src: 0});

        // fill with null events
        for i in 1..in_queues.len() {
            loser_q.push(i);
            loser_e.push(Event { time : 0, event_type: EventType::Null, src: i});
        }

        // appropriately set the source
        for (loser, ix) in ltr_walk(in_queues.len()).iter().enumerate() {
            loser_q[*ix] = loser+1;
            loser_e[*ix] = Event { time : 0, event_type: EventType::Null, src: loser+1};
        }

        let n_layers = (in_queues.len() as f32).log2().ceil() as usize;

        Merger {
            in_queues,
            n_layers,

            winner_q,

            loser_q,
            loser_e,
        }
    }


    // May return None if waiting on an input queue
    fn try_pop(&mut self) -> Option<Event> {
        if self.in_queues[self.winner_q].len() > 0 {
            return Some(self.pop());
        } else {
            return None;
        }
    }

    // blocks until it has something to return
    fn pop(&mut self) -> Event {
        // The state of this must be mostly done except for the previous winner

        // TODO handle safe_time
        // TODO handle when more than one path is empty?

        // TODO blocking wait
        println!("\nPop, previous winner {}", self.winner_q);
        let mut new_winner_e : Event = self.in_queues[self.winner_q].pop().unwrap();
        let mut new_winner_i = self.winner_q;
        //println!("  <- {:?}", new_winner_e);
        println!("  {:#?}", self.loser_q);
        //println!("  {:#?}", self.loser_e);

        // TODO change the source now
        // new_winner_e.src = self.winner_q;

        let largest_full_layer = 2_usize.pow((self.in_queues.len() as f32).log2().floor() as u32);
        let last_layer_max_i = ((self.in_queues.len() + largest_full_layer-1) % largest_full_layer + 1) * 2;
        println!("  lfl {}", largest_full_layer);
        println!("  llmi {}", last_layer_max_i);

        let mut winner_for_ix = new_winner_i;
        if new_winner_i > last_layer_max_i {
            winner_for_ix = (new_winner_i-(last_layer_max_i+1)/2)*2;
        }
        println!("  wfix {}", winner_for_ix);


        for level in (0..self.n_layers).rev() {
            println!("  Layer {}", level);

            // compute index
            let base_offset = 2_usize.pow(level as u32);
            let index = base_offset + winner_for_ix / 2_usize.pow((self.n_layers - level) as u32);
            println!("    index: {}", index);

            // skip if we're out of bounds
            if index >= self.loser_q.len() {
                println!("    skip!");
                continue;
            }

            // get current loser
            let cur_loser_i = self.loser_q[index]; // get index of queue
            let cur_loser_t = self.loser_e[index].time;

            // The current loser wins, swap with our candidate, move up
            if cur_loser_t < new_winner_e.time {
                println!("    swap!");
                // swap event
                mem::swap(&mut new_winner_e, &mut self.loser_e[index]);

                // swap queue indices
                self.loser_q[index] = new_winner_i;
                new_winner_i  = cur_loser_i;
            }
        }

        // We need this to know what path to go up next time...
        self.winner_q = new_winner_i;

        println!("  {:#?}", self.loser_q);
        //println!("  {:#?}", self.loser_e);
        // we have a winner :)
        return new_winner_e;
    }
}

#[cfg(test)]
mod test_merger {
    use crossbeam::queue::spsc;
    use crate::synchronizer::*;

    #[test]
    fn test_ltr() {
        let ix = ltr_walk(2);
        let expected = vec![1];
        assert_eq!(ix, expected);

        let ix = ltr_walk(3);
        let expected = vec![2, 1];
        assert_eq!(ix, expected);

        let ix = ltr_walk(4);
        let expected = vec![2, 1, 3];
        assert_eq!(ix, expected);

        let ix = ltr_walk(5);
        let expected = vec![4, 2, 1, 3];
        assert_eq!(ix, expected);

        let ix = ltr_walk(13);
        let expected = vec![8, 4, 9, 2, 10, 5, 11, 1, 12, 6, 3, 7];
        assert_eq!(ix, expected);
    }

    #[test]
    fn test_merge_2() {
        test_interleave(2, 5);
    }

    #[test]
    fn test_merge_4() {
        test_interleave(4, 10);
    }

    #[test]
    fn test_merge_5() {
        test_interleave(5, 10);
    }

    #[test]
    fn test_merge_7() {
        test_interleave(7, 10);
    }

    #[test]
    fn test_merge_many() {
        for n_queues in 4..20 {
            println!("{} queues =======================", n_queues);
            test_interleave(n_queues, 30);
        }
    }

    fn test_interleave(n_queues : usize, n_events : usize) {
        // Create our event queues
        let mut prod_qs = Vec::new();
        let mut cons_qs = Vec::new();

        for _ in 0..n_queues {
            let (prod, cons) = spsc::new(128);
            prod_qs.push(prod);
            cons_qs.push(cons);
        }

        // Merger
        let mut merger = Merger::new(cons_qs);

        //prod_qs
        println!("Pushing events");
        for (src, prod) in prod_qs.iter().enumerate() {
            for i in 1..n_events+1 {
                let e = Event { time: (src*1+i) as u64, src, event_type : EventType::Null };
                println!("  {} <- {:?}", i, e);
                prod.push(e).unwrap();
            }

            // close sentinel
            let e = Event { time: 100000, src, event_type : EventType::Close };
            prod.push(e).unwrap();
        }

        println!("\nPopping events");
        let mut event_count = 0;
        let mut cur_time = 0;

        while let Some(event) = merger.try_pop() {
            println!("    => {:?}", event);

            assert!(cur_time <= event.time,
                "Time invariant violated. Previous event was @{}, current event @{}",
                cur_time, event.time);
            cur_time = event.time;
            event_count += 1;
        }

        // n_q -1 starting sentinels, n_q*n_e events, 1 close event
        let expected_count = (n_queues - 1) + n_queues * n_events + 1;

        assert_eq!(event_count, expected_count,
            "Expected {} events, saw {}", expected_count, event_count);
        //assert!(false);
    }
}

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
