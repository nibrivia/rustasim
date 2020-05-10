use std::thread;

pub mod nic;

fn main() {
    println!("Setup...");

    // Create entities
    let mut src = nic::Router::new(0);
    let mut dst = nic::Router::new(2);

    let mut switch = nic::Router::new(1);

    // Connect them up!
    let hack = src.connect(&mut switch);
    switch.connect(&mut dst);

    // cheat the start of the flow
    let f = nic::Flow::new(0, 2, 200);
    for packet in f {
        hack.send(nic::Event {
                src : 1,
                time : 0,
                event_type : nic::EventType::Packet(1, packet),
            })
            .unwrap();
    }


    println!("Run...");
    let handle_src = thread::spawn(move || src.start());
    let handle_dst = thread::spawn(move || dst.start());
    let handle_switch = thread::spawn(move || switch.start());

    handle_src.join().unwrap();
    handle_dst.join().unwrap();
    handle_switch.join().unwrap();

    println!("done");
}

