use std::thread;

pub mod nic;

fn main() {
    println!("Setup...");

    // Create entities
    let mut servers = Vec::new();
    let mut ins     = Vec::new();
    let mut switch = nic::Router::new(0);

    let n_servers = 4;

    for id in 1..n_servers+1 {
        let mut s = nic::Router::new(id);
        ins.push(s.connect(&mut switch));
        servers.push(s);
    }

    // TODO find a way to create a "world" object
    for id in 1..n_servers+1 {
        let f = nic::Flow::new(id, (id)%n_servers+1, 20);
        println!("{:?}", f);
        let hack = & ins[f.src-1];
        for packet in f {
            hack.send(nic::Event {
                    src : 0,
                    time : 0,
                    event_type : nic::EventType::Packet(packet),
                })
                .unwrap();
        }
    }

    println!("Run...");

    // Start each switch in its own thread
    let mut handles = Vec::new();
    for s in servers {
        handles.push(thread::spawn(move || s.start()));
    }
    let handle_switch = thread::spawn(move || switch.start());

    // Get the results
    let mut counts = Vec::new();
    for h in handles {
        let c = h.join().unwrap();
        counts.push(c);
    }

    let swt_count = handle_switch.join().unwrap();

    println!("{:?}+{} = {}",
        counts, swt_count,
        counts.iter().sum::<u64>() + swt_count,
        );

    println!("done");
}

