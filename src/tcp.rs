//! Implements a basic version of TCP

#[derive(Debug)]
pub struct Packet {
    pub src: usize,
    pub dst: usize,
    pub seq_num: u64,
    pub size_byte: u64,

    pub ttl: u64,
    pub sent_ns: u64,
}

#[derive(Debug)]
pub struct Flow {
    pub flow_id: usize,
    pub src: usize,
    pub dst: usize,
    pub size_byte: u64,

    cwnd: u64,
    next_seq: u64,
}

const BYTES_PER_PACKET: u64 = 1500;

impl Flow {
    pub fn new(src: usize, dst: usize, n_packets: u64) -> Flow {
        Flow {
            flow_id: 0, // TODO add flow_id
            src,
            dst,

            size_byte: n_packets * BYTES_PER_PACKET,
            cwnd: 1,
            next_seq: 0,
        }
    }

    pub fn start(&self) {
        // TODO
    }

    // TODO how to timeout?
    pub fn dst_receive(&mut self, _packet : Packet) {
        // TODO process arriving packet
        // TODO send new packets
    }
}

impl Iterator for Flow {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_seq * BYTES_PER_PACKET < self.size_byte {
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

