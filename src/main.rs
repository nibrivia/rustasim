//use std::collections::BinaryHeap;
//use std::cmp::Ordering;
//use radix_heap::RadixHeapMap;
//use std::collections::VecDeque;

pub mod nic;
pub mod scheduler;

fn main() {
    let mut s = scheduler::Scheduler::new();

    let f = nic::Flow::new();
    for packet in f {
        s.call_in(0, scheduler::EventType::NICRx{nic: 0, packet});
    }
    s.call_in(0, scheduler::EventType::NICEnable{nic: 0});


    s.run();

    println!("done");
}

