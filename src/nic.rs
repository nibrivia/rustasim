use std::collections::VecDeque;
use radix_heap::RadixHeapMap;
use crate::scheduler;

pub struct Packet {
    src: u32,
    dst: u32,
    seq_num: u64,
    size_byte: u64,

    ttl: u32,
    sent_ns: u64,
}

pub struct Flow {
    src: u32,
    dst: u32,

    size_byte: u64,
    cwnd: u32,
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
            next_seq: 0
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


pub struct NIC {
    latency_ns: u64,
    ns_per_byte: u64,
    enabled: bool,
    pub count: u64,
    queue: VecDeque<Packet>,
}

impl NIC {
    pub fn new() -> NIC {
        NIC {
            latency_ns: 10,
            ns_per_byte: 1,
            enabled: false,
            count : 0,
            queue : VecDeque::new(),
        }
    }

    pub fn enq(&mut self, time: u64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, p: Packet) {
        //println!("Received packet #{}!", p.seq_num);

        self.queue.push_back(p);
        self.count += 1;

        // attempt send
        self.send(time, event_queue, false);
    }

    pub fn send(&mut self, time: u64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, enable: bool) {
        self.enabled = self.enabled | enable;

        if !self.enabled || self.queue.len() == 0 {
            return
        }

        let packet = self.queue.pop_front().unwrap();
        //println!("Sending packet #{}", packet.seq_num);
        self.enabled = false;


        //scheduler.call_in(1500, EventType::NICEnable{nic: 0});
        //scheduler.call_in(1510, EventType::NICRx{nic: 0, packet});
        event_queue.push(-(time as i64+1500), scheduler::Event{time: time+1500, event_type: scheduler::EventType::NICEnable{nic: 0}});
        event_queue.push(-(time as i64+1510), scheduler::Event{time: time+1510, event_type: scheduler::EventType::NICRx{nic: 0, packet}});
        //let reenable = || {self.send(true)};
        //self.scheduler.call_in(1500, Box::new(reenable));
    }
}

