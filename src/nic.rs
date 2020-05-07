use std::collections::VecDeque;
use radix_heap::RadixHeapMap;
use crate::scheduler;

pub type SizeByte = u64;

#[derive(Debug)]
pub struct Packet {
    src: ServerID,
    dst: ServerID,
    seq_num: u64,
    size_byte: SizeByte,

    ttl: u32,
    sent_ns: i64,
}

#[derive(Debug)]
pub struct Flow {
    src: ServerID,
    dst: ServerID,

    size_byte: SizeByte,
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

pub trait Receiver {
    fn receive(&mut self, time: i64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, packet: Packet);
}

#[derive(Debug)]
pub struct NIC {
    // fundamental properties
    latency_ns: i64,
    ns_per_byte: i64,

    // operational management
    enabled: bool,
    queue: VecDeque<Packet>,

    // stats
    pub count: u64,
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
}

impl Receiver for NIC {
    fn receive(&mut self, time: i64, event_queue: &mut RadixHeapMap<i64, scheduler::Event>, p: Packet) {
        //println!("Received packet #{}!", p.seq_num);

        self.queue.push_back(p);
        self.count += 1;

        // attempt send
        self.send(time, event_queue, false);
    }
}

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

    servers: Vec<Server>,

    output_queues: Vec<NIC>,
}

impl ToR {
    pub fn new(tor_id : ToRID, n_servers: usize) -> ToR {
        let mut servers : Vec<Server> = Vec::new();
        let mut output_queues : Vec<NIC> = Vec::new();

        // TODO make this work when >1 ToR
        for server_id in 0..n_servers {
            servers.push(Server::new(server_id));
            output_queues.push(NIC::new());
        }

        ToR {
            tor_id,
            servers,
            output_queues,
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

