//! Implements a basic version of TCP

use crate::Time;

/// Contains the timeout time, flow_id and seq_num
pub type Timeout = (Time, usize, usize);

/// This is based on typical MTUs.
pub const BYTES_PER_PACKET: u64 = 1500;

/// Smallest RTO
pub const MIN_RTO: Time = 2_000_000;

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
    pub seq_num: usize,

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
    next_seq: usize,

    acked: Vec<bool>,
}

impl Flow {
    /// Creates a new flow
    pub fn new(flow_id: usize, src: usize, dst: usize, size_byte: u64) -> Flow {
        Flow {
            flow_id,
            src,
            dst,

            size_byte,
            cwnd: 2,
            next_seq: 0,
            acked: Vec::new(),
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
    pub fn src_receive(&mut self, packet: Packet) -> (Vec<Packet>, Vec<Timeout>) {
        // mark packet as ack'd
        self.acked[packet.seq_num as usize] = true;

        // next packets to send
        let mut packets = Vec::new();
        let mut timeouts = Vec::new();
        if let Some(p) = self.next() {
            timeouts.push((self.rto(), self.flow_id, p.seq_num));
            packets.push(p);
        }

        (packets, timeouts)
    }

    /// To be called on a timeout
    pub fn timeout(&mut self, seq_num: usize) -> (Vec<Packet>, Vec<Timeout>) {
        if !self.acked[seq_num] {
            println!(
                "{} rx timeout for #{}, acked: {}",
                self.flow_id, seq_num, self.acked[seq_num]
            );

            // TODO retransmit *something*?
            (vec![], vec![])
        } else {
            (vec![], vec![])
        }
    }
}

impl Iterator for Flow {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO manage retransmits
        if self.next_seq as u64 * BYTES_PER_PACKET < self.size_byte {
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

            // hasn't been acked yet...
            self.acked.push(false);

            Some(p)
        } else {
            None
        }
    }
}
