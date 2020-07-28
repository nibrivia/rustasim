//! Implements a basic version of TCP

use crate::Time;
use std::collections::VecDeque;

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

// flow_id, src, dst, size_bytes
/// Short description of a flow object
pub type FlowDesc = (usize, usize, u64);

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

    start: Time,

    cwnd: usize,
    outstanding: usize,
    n_acked: u64,

    next_seq: usize,
    acked: Vec<bool>,
    rtx_queue: VecDeque<usize>,
}

impl Flow {
    /// Creates a new flow
    pub fn new(flow_id: usize, src: usize, dst: usize, size_byte: u64) -> Flow {
        Flow {
            flow_id,
            src,
            dst,

            size_byte,
            start: 0,

            cwnd: 5,
            outstanding: 0,
            n_acked: 0,

            next_seq: 0,
            acked: Vec::new(),
            rtx_queue: VecDeque::new(),
        }
    }

    /// Computes the current timeout
    fn rto(&self) -> Time {
        MIN_RTO
    }

    /// Generates the packet with the given sequence number for this flow
    fn gen_packet(&self, seq_num: usize) -> Packet {
        Packet {
            src: self.src,
            dst: self.dst,
            seq_num,
            size_byte: BYTES_PER_PACKET,

            flow_id: self.flow_id,
            is_ack: false,

            //ttl: 10,
            sent_ns: 0,
        }
    }

    /// Starts the flow, returns the initial burst of packets to send
    pub fn start(&mut self, time: Time) -> (Vec<Packet>, Vec<Timeout>) {
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

        self.outstanding = packets.len();
        self.start = time;

        (packets, timeouts)
    }

    /// Receives an ack and returns the appropriate packets to sene
    pub fn src_receive(&mut self, time: Time, packet: Packet) -> (Vec<Packet>, Vec<Timeout>) {
        // if we've already acked the packet, do nothing
        if !self.acked[packet.seq_num] {
            self.outstanding -= 1;
            self.n_acked += 1;
            if self.n_acked * BYTES_PER_PACKET >= self.size_byte {
                println!(
                    "{src},{dst},{start},{end},{size_byte},{fct}",
                    src = self.src,
                    dst = self.dst,
                    size_byte = self.size_byte,
                    start = self.start,
                    end = time,
                    fct = time - self.start,
                );
            }
            //self.cwnd += 1/self.cwnd;
        }

        // mark packet as ack'd
        self.acked[packet.seq_num] = true;

        // TODO rto
        // TODO cwnd

        // next packets to send
        let mut packets = Vec::new();
        let mut timeouts = Vec::new();
        for _ in self.outstanding..self.cwnd {
            if let Some(p) = self.next() {
                timeouts.push((self.rto(), self.flow_id, p.seq_num));
                packets.push(p);
            } else {
                break;
            }
        }

        self.outstanding += packets.len();
        (packets, timeouts)
    }

    /// To be called on a timeout
    pub fn timeout(&mut self, seq_num: usize) -> (Vec<Packet>, Vec<Timeout>) {
        if !self.acked[seq_num] {
            self.outstanding -= 1;
            self.rtx_queue.push_back(seq_num);

            let mut packets = Vec::new();
            let mut timeouts = Vec::new();
            for _ in self.outstanding..self.cwnd {
                if let Some(p) = self.next() {
                    timeouts.push((self.rto(), self.flow_id, p.seq_num));
                    packets.push(p);
                } else {
                    break;
                }
            }

            self.outstanding += packets.len();
            (packets, timeouts)
        } else {
            (vec![], vec![])
        }
    }
}

impl Iterator for Flow {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        // First retransmits...
        while let Some(seq_num) = self.rtx_queue.pop_front() {
            // we might have gotten ack'd since being added to the queue, if so, try again
            if self.acked[seq_num] {
                continue;
            }

            // Found something to retransmit, we're done
            //println!("Flow {} rtx #{}", self.flow_id, seq_num);
            return Some(self.gen_packet(seq_num));
        }

        // then, normal packets
        if self.next_seq as u64 * BYTES_PER_PACKET < self.size_byte {
            // next packet
            let p = self.gen_packet(self.next_seq);

            // update for next one
            self.next_seq += 1;

            // hasn't been acked yet...
            self.acked.push(false);

            Some(p)
        } else {
            // done
            None
        }
    }
}
