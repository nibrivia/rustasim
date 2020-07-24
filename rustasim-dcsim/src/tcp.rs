//! Implements a basic version of TCP

use crate::Time;

/// Contains the timeout time, flow_id and seq_num
pub type Timeout = (Time, usize, u64);

/// This is based on typical MTUs.
pub const BYTES_PER_PACKET: u64 = 1500;

/// Smallest RTO
pub const MIN_RTO: Time = 1_000_000;

/// Describes a TCP/IP packet
///
/// The two protocols are merged together. Although not technically accurate, it is rare for TCP
/// packets to be split, at least not in datacenter networks.
#[derive(Debug)]
pub struct Packet {
    /// ID of the packet's source
    pub src: usize,

    /// ID of the packet's destination
    pub dst: usize,

    /// Packet's TCP sequence number
    pub seq_num: u64,

    /// Packet's size in bytes
    pub size_byte: u64,

    /// Whether this is a TCP ACK
    pub is_ack: bool,

    /// The flow ID this packet belongs to
    pub flow_id: usize,

    ///// How many more hops can this packet go?
    //pub ttl: usize,
    /// When was this packet originally created, in ns
    pub sent_ns: Time,
}

/// Flow data structure
#[derive(Debug)]
pub struct Flow {
    /// ID of the flow
    pub flow_id: usize,

    /// ID of the originating server
    pub src: usize,

    /// ID of the destination server
    pub dst: usize,
    size_byte: u64,

    cwnd: u64,
    next_seq: u64,
}

impl Flow {
    /// Creates a new flow
    pub fn new(flow_id: usize, src: usize, dst: usize, size_byte: u64) -> Flow {
        Flow {
            flow_id,
            src,
            dst,

            size_byte,
            cwnd: 4,
            next_seq: 0,
        }
    }

    fn rto(&self) -> Time {
        MIN_RTO // 5ms
    }

    /// Starts the flow, returns the initial burst of packets to send
    pub fn start(&mut self) -> (Vec<Packet>, Vec<Timeout>) {
        let mut packets = Vec::new();
        let mut timeouts = Vec::new();
        for _ in 0..self.cwnd {
            match self.next() {
                None => break,
                Some(p) => {
                    timeouts.push((self.rto(), self.flow_id, p.seq_num));
                    packets.push(p);
                }
            }
        }

        (packets, timeouts)
    }

    /// Receives an ack and returns the appropriate packets to sene
    pub fn src_receive(&mut self, _packet: Packet) -> (Vec<Packet>, Vec<Timeout>) {
        let mut packets = Vec::new();
        if let Some(p) = self.next() {
            packets.push(p);
        }

        (packets, Vec::new())
    }

    /// To be called on a timeout
    pub fn timeout(&mut self, seq_num: u64) -> (Vec<Packet>, Vec<Timeout>) {
        println!("{} rx timeout for #{}", self.flow_id, seq_num);
        (vec![], vec![])
    }
}

impl Iterator for Flow {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO manage retransmits
        if self.next_seq * BYTES_PER_PACKET < self.size_byte {
            let p = Packet {
                src: self.src,
                dst: self.dst,
                seq_num: self.next_seq,
                size_byte: BYTES_PER_PACKET,

                flow_id: self.flow_id,
                is_ack: false,

                //ttl: 10,
                sent_ns: 0,
            };
            self.next_seq += 1;
            Some(p)
        } else {
            None
        }
    }
}
