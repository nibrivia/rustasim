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

    // TODO find a way to create a "world" object
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

    // Start each switch in its own thread
    let handle_src = thread::spawn(move || src.start());
    let handle_dst = thread::spawn(move || dst.start());
    let handle_switch = thread::spawn(move || switch.start());

    // Get the results
    let src_count = handle_src.join().unwrap();
    let dst_count = handle_dst.join().unwrap();
    let swt_count = handle_switch.join().unwrap();

    println!("{}+{}+{} = {}",
        src_count, swt_count, dst_count,
        src_count+ swt_count+ dst_count
        );

    println!("done");
}

