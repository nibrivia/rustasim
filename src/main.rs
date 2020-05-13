use std::thread;
use ringbuf::*;

pub mod nic;
pub mod tcp;
pub mod synchronizer;

use crate::nic::*;
use crate::tcp::*;
use crate::synchronizer::*;


struct World {
    racks : Vec<Router>,
    chans : Vec<Producer<Event>>,
}

impl World {
    fn new(n_racks : usize) -> World {

        // Create the racks and connect them all up
        let mut racks = Vec::new();
        for id in 1..n_racks {
            let mut r = Router::new(id);
            for id2 in 1..id {
                r.connect(racks.get_mut(id2).unwrap());
            }
            racks.push(r);
        }

        // TODO initiate backbone switches

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

    fn start(self) -> Vec<u64> {
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

    // Create entities
    // TODO create separate racks
    let mut servers: Vec<Router> = Vec::new();
    //let mut switch0 = Router::new(0);

    let n_servers = 8;

    for id in 0..n_servers {
        let mut s = Router::new(id);
        for id2 in 0..id {
            s.connect(servers.get_mut(id2).unwrap());
        }
        servers.push(s);
    }

    // TODO find a way to create a "world" object
    for src in 0..n_servers {
        for dst in 0..n_servers-1 {
            // skip self->self
            if src == dst {
                continue;
            }

            // create flow
            let f = Flow::new(src, (dst) % n_servers, 40);

            // schedule on source
            let mut packets = Vec::new();
            for packet in f {
                packets.push(Event {
                    src: dst,
                    time: 0,
                    event_type: EventType::Packet(packet),
                });
            }
            let dst_server = servers.get_mut(dst).unwrap();
            dst_server.init_queue(src, packets);
        }
    }

    println!("Run...");

    // Start each switch in its own thread
    let mut handles = Vec::new();
    for s in servers {
        handles.push(thread::spawn(move || s.start()));
    }

    // Get the results
    let mut counts = Vec::new();
    for h in handles {
        let c = h.join().unwrap();
        counts.push(c);
    }

    println!("{:?} = {}", counts, counts.iter().sum::<u64>());

    println!("done");
}
