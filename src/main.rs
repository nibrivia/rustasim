use std::thread;
use ringbuf::*;

pub mod nic;
pub mod tcp;
pub mod synchronizer;

use crate::nic::*;
use crate::tcp::*;
use crate::synchronizer::*;

// TODO pass in limits as arguments
//                  s   ms  us  ns
const DONE: u64 = 001_000_000_000;

struct World {
    racks : Vec<Router>,
    chans : Vec<Producer<Event>>,
}

impl World {
    fn new(n_racks : usize) -> World {

        // Create the racks and connect them all up
        let mut racks = Vec::new();
        for id in 1..n_racks+1 {
            let mut r = Router::new(id);
            for id2 in 1..id {
                r.connect(racks.get_mut(id2-1).unwrap());
            }
            racks.push(r);
        }

        // flows
        for src in 1..n_racks+1 {
            for dst in 1..n_racks+1 {
                // skip self->self
                if src == dst {
                    continue;
                }

                // create flow
                let f = Flow::new(src, dst, 40);

                // schedule on source
                let mut packets = Vec::new();
                for packet in f {
                    packets.push(Event {
                        src: dst,
                        time: 0,
                        event_type: EventType::Packet(packet),
                    });
                }
                let dst_rack = racks.get_mut(dst-1).unwrap();
                dst_rack.init_queue(src, packets);
            }
        }

        // TODO backbone switches

        // conect world
        let mut chans = Vec::new();
        for r in &mut racks {
            chans.push(r.connect_world());
        }

        // reuturn world
        World {
            racks,
            chans
        }
    }

    fn start(mut self) -> Vec<u64> {
        // Tell everyone when the end is
        for c in self.chans.iter_mut() {
            c.push(Event {
                time: DONE,
                src: 0,
                event_type: EventType::Close,
            }).unwrap();
        }

        // Start each rack in its own thread
        let mut handles = Vec::new();
        for r in self.racks {
            handles.push(thread::spawn(move || r.start()));
        }

        // Get the results
        let mut counts = Vec::new();
        for h in handles {
            let c = h.join().unwrap();
            counts.push(c);
        }

        return counts;
    }
}

fn main() {
    println!("Setup...");

    let world = World::new(14);

    println!("Run...");
    let counts = world.start();

    println!("{:?} = {}", counts, counts.iter().sum::<u64>());
    println!("done");
}
