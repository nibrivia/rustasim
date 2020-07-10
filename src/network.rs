//! Datacenter network model

use crate::World;
use crossbeam_queue::spsc::Producer;
use std::time::Instant;

pub mod router;
pub mod routing;
pub mod server;
pub mod tcp;

const Q_SIZE: usize = 1 << 12;

/// Datacenter network model events
pub enum NetworkEvent {
    /// Flow start
    Flow(tcp::Flow),

    /// Packet arrival
    Packet(tcp::Packet),
}

impl std::fmt::Debug for NetworkEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NetworkEvent::Flow(_) => "Flow",
            NetworkEvent::Packet(packet) => {
                if packet.is_ack {
                    "Ack"
                } else {
                    "Packet"
                }
            }
        })
    }
}

pub type ModelEvent = crate::engine::Event<NetworkEvent>;

/// Device types
#[derive(Debug)]
pub enum Device {
    Router,
    Server,
}

/// A standard interface for connecting devices of all types
// TODO change this API, connect(a, b) function, connectable just has functions for giving and
// getting queues.
pub trait Connectable {
    fn id(&self) -> usize;
    fn flavor(&self) -> Device;

    fn connect(&mut self, other: impl Connectable);
    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent>;
}

pub fn build_network() {
    // TODO pass in time_limit, n_threads as arguments

    //let time_limit: u64 = 1_000_000_000;
    //                      s  ms  us  ns
    let time_limit: u64 = 000_111_111_000;

    let n_racks = 10;

    println!("Setup...");
    let world = World::new(n_racks);

    println!("Run...");
    let start = Instant::now();
    let counts = world.start(num_cpus::get() - 1, time_limit);
    let duration = start.elapsed();

    let n_thread = counts.len();
    let n_cpus = std::cmp::min(num_cpus::get() - 1, n_thread);

    // each ToR sends to n_racks-1 racks and n_racks-1 servers
    // each server (n_racks^2) is connected to 1 ToR
    let n_links = (n_racks * 2 * (n_racks - 1) + (n_racks * (n_racks - 1))) as u64;

    // stats...
    let sum_count = counts.iter().sum::<u64>();
    let ns_per_count: f64 = if sum_count > 0 {
        1000. * duration.as_nanos() as f64 / sum_count as f64
    } else {
        0.
    };

    // each link is 8Gbps, time_limit/1e9 is in seconds which is how much we simulated
    // divide by the time it took us -> simulation bandwidth
    let gbps = (n_links * 8 * time_limit) as f64 / 1e9 / duration.as_secs_f64();

    println!(
        "= {} in {:.3}s. {} actors, {} cores",
        sum_count,
        duration.as_secs_f32(),
        n_thread,
        n_cpus,
    );
    println!(
        "  {:.3}M count/sec, {:.3}M /actors, {:.3}M /cpu",
        (1e6 / ns_per_count as f64),
        (1e6 / (ns_per_count * n_thread as f64)),
        (1e6 / (ns_per_count * n_cpus as f64)),
    );
    println!(
        "  {:.1} ns/count, {:.1} ns/actor, {:.1} ns/cpu",
        ns_per_count / 1000. as f64,
        ns_per_count * n_thread as f64 / 1000.,
        ns_per_count * n_cpus as f64 / 1000.
    );
    println!(
        "  {:.3} gbps, {:.3} gbps/actor ({} links total)",
        gbps,
        (gbps / n_thread as f64),
        n_links
    );

    println!("done");
}
