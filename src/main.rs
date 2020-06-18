use std::time::Instant;

use rustasim::World;

fn main() {
    println!("Setup...");

    let n_thread = 7;
    let world = World::new(n_thread);

    println!("Run...");

    let start = Instant::now();
    let counts = world.start();
    let duration = start.elapsed();

    let sum_count = counts.iter().sum::<u64>();
    let mut ns_per_count = 0;
    if sum_count > 0 {
        ns_per_count = 1000 * duration.as_nanos() / sum_count as u128;
    }
    let gbps = ((n_thread*(n_thread-1) * 8) as f64) * (rustasim::DONE as f64)/1e9 / duration.as_secs_f64();

    println!("= {} / {}s", sum_count, duration.as_secs_f32());
    println!("  {}M count/sec, {}M count/sec/thread",
        (1e6 / ns_per_count as f64) as u64,
        (1e6 / (ns_per_count * n_thread as u128) as f64) as u64);
    println!("  {} ns/count, {} ns/count/thread",
        ns_per_count /1000, ns_per_count * n_thread as u128 / 1000);
    println!("  {} gbps, {} gbps/thread", gbps as u64, (gbps/n_thread as f64) as u64);

    println!("done");
}
