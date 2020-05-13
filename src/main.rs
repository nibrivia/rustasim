use std::thread;

pub mod nic;
pub mod synchronizer;

use crate::nic::*;
use crate::synchronizer::*;

fn main() {
    println!("Setup...");

    // Create entities
    // TODO create separate racks
    let mut servers: Vec<Router> = Vec::new();
    //let mut switch0 = Router::new(0);

    let n_servers = 14;

    for id in 0..n_servers {
        let mut s = Router::new(id);
        for id2 in 0..id {
            s.connect(servers.get_mut(id2).unwrap());
        }
        servers.push(s);
    }

    // TODO find a way to create a "world" object
    for src in 0..n_servers {
        for dst in 0..n_servers {
            // skip self->self
            if src == dst {
                continue;
            }

            // create flow
            let f = Flow::new(src, (dst) % n_servers, 40);
            println!("{:?}", f);

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
