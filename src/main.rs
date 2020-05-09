//use std::collections::BinaryHeap;
//use std::cmp::Ordering;
//use radix_heap::RadixHeapMap;
//use std::collections::VecDeque;

pub mod nic;
pub mod scheduler;

fn main() {
    println!("Setup...");

    /*let mut n = scheduler::Network::new();

    let f = nic::Flow::new();
    for packet in f {
        n.call_in(0, scheduler::EventType::NICRx{nic: 0, packet});
    }
    n.call_in(0, scheduler::EventType::NICEnable{nic: 0});
    */


    println!("Run...");
    //n.run();

    println!("done");
}

