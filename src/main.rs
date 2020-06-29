use std::time::Instant;

use rustasim::World;

fn main() {
    // TODO pass in time_limit, n_threads as arguments

    //let time_limit: u64 = 1_000_000_000;
    //                      s  ms  us  ns
    let time_limit: u64 = 000_011_111_000;

    let n_racks = 8;

    println!("Setup...");
    let world = World::new(n_racks);

    println!("Run...");
    let start = Instant::now();
    let counts = world.start(time_limit);
    let duration = start.elapsed();

    let n_thread = counts.len();

    // each ToR sends to n_racks-1 racks and n_racks servers
    // each server (n_racks^2) is connected to 1 ToR
    let n_links = (n_racks * (n_racks - 1 + n_racks) + n_racks) as u64;

    // stats...
    let sum_count = counts.iter().sum::<u64>();
    let ns_per_count = if sum_count > 0 {
        1000 * duration.as_nanos() / sum_count as u128
    } else {
        0
    };

    // each link is 8Gbps, time_limit/1e9 is in seconds which is how much we simulated
    // divide by the time it took us -> simulation bandwidth
    let gbps = (n_links * 8 * time_limit) as f64 / 1e9 / duration.as_secs_f64();

    println!("= {} / {}s", sum_count, duration.as_secs_f32());
    println!(
        "  {}M count/sec, {}M count/sec/thread",
        (1e6 / ns_per_count as f64) as u64,
        (1e6 / (ns_per_count * n_thread as u128) as f64) as u64
    );
    println!(
        "  {} ns/count, {} ns/count/thread",
        ns_per_count / 1000,
        ns_per_count * n_thread as u128 / 1000
    );
    println!(
        "  {} gbps, {} gbps/thread",
        gbps as u64,
        (gbps / n_thread as f64) as u64
    );

    println!("done");
}
