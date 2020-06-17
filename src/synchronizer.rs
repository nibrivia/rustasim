use std::mem;
use std::cmp::Ordering;
use crossbeam::queue::spsc;

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
pub struct Merger {
    // the input queues
    in_queues : Vec<spsc::Consumer<Event>>,
    n_layers : usize,

    paths : Vec<usize>,

    // the queue to pull from
    winner_q : usize,
    safe_time: u64,
    stalled : bool,

    // the loser queue
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
    pub fn new(in_queues : Vec<spsc::Consumer<Event>>) -> Merger {
        let mut loser_e = Vec::new();
        let winner_q = 0;

        // index 0 never gets used
        loser_e.push(Event { time : 0, event_type: EventType::Null, src: 0});

        // TODO hacky
        // fill with placeholders
        for i in 1..in_queues.len() {
            loser_e.push(Event { time : 0, event_type: EventType::Null, src: i});
        }

        // appropriately set the source
        for (loser, ix) in ltr_walk(in_queues.len()).iter().enumerate() {
            loser_e[*ix] = Event { time : 0, event_type: EventType::Null, src: loser+1};
        }

        // helfpul number
        let n_queues = in_queues.len();
        let n_layers = (n_queues as f32).log2().ceil() as usize;
        let largest_full_layer = 2_usize.pow((n_queues as f32).log2().floor() as u32);
        let last_layer_max_i = ((n_queues + largest_full_layer-1) % largest_full_layer + 1) * 2;
        let offset = (last_layer_max_i+1)/2;

        let mut paths = Vec::new();
        for ix in 0..n_queues {
            let v_ix;
            if ix > last_layer_max_i {
                v_ix = (ix-offset)*2;
            } else {
                v_ix = ix;
            }

            let mut index = 0;

            assert!(n_layers > 0);
            for level in (0..n_layers).rev() {
                // compute index
                // TODO pre-compute?
                let base_offset = 2_usize.pow(level as u32);
                index = base_offset + v_ix / 2_usize.pow((n_layers - level) as u32);

                // skip if we're out of bounds
                if index >= loser_e.len() {
                    continue;
                }
                break;
            }

            paths.push(index);
        };

        Merger {
            in_queues,
            n_layers,

            winner_q,
            safe_time : 0,
            stalled: false,

            paths,

            loser_e,
        }
    }

    // May return None if waiting on an input queue
    fn _try_pop(&mut self) -> Option<Event> {
        if self.in_queues[self.winner_q].len() > 0 {
            return self.next();
        } else {
            return None;
        }
    }
}

impl Iterator for Merger {
    type Item = Event;

    // blocks until it has something to return
    fn next(&mut self) -> Option<Self::Item> {
        // The state of this must be mostly done except for the previous winner
        loop {
            // get the path up
            let mut index = self.paths[self.winner_q];
            let q = &self.in_queues[self.winner_q]; // avoids regularly indexing into that vec

            // TODO handle safe_time
            // TODO handle when more than one path is empty?

            // get the new candidate
            let mut new_winner_e = match q.pop() {
                Err(_) => {
                    if !self.stalled {
                        // return Stalled event
                        self.stalled = true;
                        return Some(Event { time : self.safe_time, src : self.winner_q, event_type: EventType::Stalled });
                    } else {
                        // blocking wait
                        q.wait();
                        //while q.len() == 0 {}
                        self.stalled = false;
                        q.pop().unwrap()
                    }
                }
                Ok(event) => {
                    self.stalled = false;
                    event
                }
            };

            // change the source id->ix now
            new_winner_e.src = self.winner_q;

            // go up our path, noting the loser as we go
            while index != 0 {
                // get current loser
                let cur_loser_t = self.loser_e[index].time;

                // The current loser wins, swap with our candidate, move up
                if cur_loser_t < new_winner_e.time {
                    // swap event
                    mem::swap(&mut new_winner_e, &mut self.loser_e[index]);
                }

                index /= 2;
            }

            // We need this to know what path to go up next time...
            self.winner_q = new_winner_e.src;

            // We need this to return events even if we don't have new events coming in...
            self.safe_time = new_winner_e.time;

            // Null events are only useful for us
            if let EventType::Null = new_winner_e.event_type {
                continue;
            }

            return Some(new_winner_e);
        }
    }
}

#[cfg(test)]
mod test_merger {
    use crossbeam::queue::spsc;
    use crate::synchronizer::*;
    use std::{thread, time};

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
        test_pushpop(2, 5);
    }

    #[test]
    fn test_merge_4() {
        test_interleave(4, 10);
        test_pushpop(4, 10);
    }

    #[test]
    fn test_merge_5() {
        test_interleave(5, 10);
        test_pushpop(5, 10);
    }

    #[test]
    fn test_merge_7() {
        test_interleave(7, 10);
        test_pushpop(7, 10);
    }

    #[test]
    fn test_merge_many_interleave() {
        for n_queues in 3..20 {
            println!("{} queues =======================", n_queues);
            test_interleave(n_queues, n_queues+5);
        }
    }

    #[test]
    fn test_merge_many_threads() {
        for n_queues in 3..20 {
            println!("{} queues =======================", n_queues);
            test_pushpop(n_queues, n_queues+5);
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
                let e = Event { time: (src*1+i) as u64, src, event_type : EventType::Close };
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

        while let Some(event) = merger._try_pop() {
            println!("    => {:?}", event);

            assert!(cur_time <= event.time,
                "Time invariant violated. Previous event was @{}, current event @{}",
                cur_time, event.time);
            cur_time = event.time;
            event_count += 1;
        }

        // n_q*n_e events, 1 close event
        let expected_count = n_queues * n_events + 1;

        assert_eq!(event_count, expected_count,
            "Expected {} events, saw {}", expected_count, event_count);
    }

    fn test_pushpop(n_queues : usize, n_events : usize) {
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

        // checker vars

        println!("Multithreaded pushing and popping events");
        let handle = thread::spawn(move || {
            // n_q*n_e events, 1 close event
            let expected_count = n_queues * n_events + 1;

            let mut event_count = 0;
            let mut cur_time = 0;

            // pop as much as we can
            while event_count < expected_count {
                let event = merger.next().unwrap();
                //println!("    => {:?}", event);
                if let EventType::Stalled = event.event_type {
                    continue;
                }

                assert!(cur_time <= event.time,
                    "Time invariant violated. Previous event was @{}, current event @{}",
                    cur_time, event.time);
                cur_time = event.time;
                event_count += 1;
            }

            if let Some(event) = merger._try_pop() {
                assert!(false, "Merger should not have any more events, got {:?}", event);
            }

            assert_eq!(event_count, expected_count,
                "Expected {} events, saw {}", expected_count, event_count);
        });

        for (src, prod) in prod_qs.iter().enumerate().rev() {
            for i in 1..n_events+1 {
                let e = Event { time: (src*1+4*i) as u64, src, event_type : EventType::Close };
                //println!("  {} <- {:?}", i, e);
                prod.push(e).unwrap();
                thread::sleep(time::Duration::from_micros(1));
            }

            // close sentinel
            let e = Event { time: 100000, src, event_type : EventType::Close };
            prod.push(e).unwrap();
        }

        handle.join().unwrap();
    }
}

