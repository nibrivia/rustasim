use std::thread;

pub mod nic;
pub mod synchronizer;

use crate::synchronizer::*;
use crate::nic::*;

fn main() {
    println!("Setup...");

    // Create entities
    let mut servers = Vec::new();
    let mut ins     = Vec::new();
    let mut switch0 = Router::new(0);
    //let mut switch1 = Router::new(100);
    //switch0.connect(&mut switch1);

    let n_servers = 10;

    for id in 1..n_servers+1 {
        let mut s = Router::new(id);
        ins.push(s.connect(&mut switch0));
        //s.connect(&mut switch1);
        servers.push(s);
    }

    // TODO find a way to create a "world" object
    for id in 1..n_servers+1 {
        let f = Flow::new(id, (id)%n_servers+1, 20);
        println!("{:?}", f);
        let hack = & ins[f.src-1];
        for packet in f {
            hack.send(Event {
                    src : 0,
                    time : 0,
                    event_type : EventType::Packet(packet),
                })
                .unwrap();
        }
    }

    println!("Run...");

    // Start each switch in its own thread
    let handle_switch0 = thread::spawn(move || switch0.start());
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

    let swt0_count = handle_switch0.join().unwrap();
    let swt1_count = 0;

    println!("{:?}+{}+{} = {}",
        counts, swt0_count, swt1_count,
        counts.iter().sum::<u64>() + swt0_count + swt1_count,
        );

    println!("done");
}

