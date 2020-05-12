use std::thread;

pub mod nic;
pub mod synchronizer;

use crate::synchronizer::*;
use crate::nic::*;

fn main() {
    println!("Setup...");

    // Create entities
    // TODO create separate racks
    let mut servers : Vec<Router> = Vec::new();
    let mut ins     = Vec::new();
    //let mut switch0 = Router::new(0);
    //let mut switch1 = Router::new(100);
    //switch0.connect(&mut switch1);

    let n_servers = 14;

    for id in 0..n_servers {
        let mut s = Router::new(id);
        for id2 in 0..id {
            if id == 2 && id2 == 0 {
                let t = servers.get_mut(id2).unwrap().connect(&mut s);
                ins.insert(0, t);
                continue;
            } else {
                let t = s.connect(servers.get_mut(id2).unwrap());
                if id2 == 0 || id == 2 && id2 == 1{
                    ins.push(t);
                }
            }
        }
        //s.connect(&mut switch1);
        servers.push(s);
    }

    // TODO find a way to create a "world" object
    for src in 0..n_servers {
        for dst in 0..n_servers {
            if src == dst {
                continue
            }
            let f = Flow::new(src, (dst)%n_servers, 40);
            println!("{:?}", f);
            let hack = & ins[f.src];
            for packet in f {
                hack.send(Event {
                        src : dst,
                        time : 0,
                        event_type : EventType::Packet(packet),
                    })
                    .unwrap();
            }
        }
    }

    println!("Run...");

    // Start each switch in its own thread
    //let handle_switch0 = thread::spawn(move || switch0.start());
    //let handle_switch1 = thread::spawn(move || switch1.start());
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

    let swt0_count = 0;
    let swt1_count = 0;

    println!("{:?}+{}+{} = {}",
        counts, swt0_count, swt1_count,
        counts.iter().sum::<u64>() + swt0_count + swt1_count,
        );

    println!("done");
}

