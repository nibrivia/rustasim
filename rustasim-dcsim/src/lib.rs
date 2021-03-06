#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
//! Datacenter network model

// I like to have many small files
mod router;
mod routing;
mod server;
mod tcp;

// but it's much easier to use if they're not in different modules
pub use self::router::*;
pub use self::routing::*;
pub use self::server::*;
pub use self::tcp::*;

use csv::ReaderBuilder;
use rustasim::spsc::Producer;
use rustasim::{start, Advancer, Event, EventType};
use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::time::Instant;

/// Size for the internal event queue
const Q_SIZE: usize = 1 << 14;

/// Convenience alias for time type
pub type Time = u64;

/// Convenience alias for the simulation result
pub type ActorResult = u64;

/// Shorthand for the event types
pub type ModelEvent = Event<Time, NetworkEvent>;

/// Simulation parameters
#[derive(Debug)]
pub struct SimConfig {
    /// Simulation end, in ns
    pub time_limit: Time,

    /// Datacenter topology type
    pub topology: Topology,

    /// Flow file
    pub flow_file: String,

    /// Link bandwidth
    pub bandwidth_gbps: u64,

    /// Server<>ToR<>* latency
    pub latency_ns: Time,
    // ToR<>* latency
    //pub tor_out_latency_ns: Time,
}

/// Topology types
#[derive(Debug, Deserialize)]
pub enum Topology {
    /// 3 tiered CLOS(u, d) with `u` uplinks and `d` downlinks
    CLOS(usize, usize),

    /// FullyConnected(n) All `n` racks are connected to all other racks, `n-1` servers/rack
    FullyConnected(usize),
    //Expander(u64),
    //Rotor,
    //Opera,
}

/// Datacenter network model events
pub enum NetworkEvent {
    /// Flow start
    Flow(FlowDesc),

    /// Packet arrival
    Packet(Packet),

    /// Server timeout, server needs to check itself for timeouts
    Timeout,
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
            NetworkEvent::Timeout => "Timeout",
        })
    }
}

/// Computes the final transmit and receive times for a packet
pub fn tx_rx_time(
    cur_time: Time,
    packet_size_bytes: u64,
    latency: Time,
    bandwidth_gbps: u64,
) -> (Time, Time) {
    let tx_time = cur_time + 8 * packet_size_bytes / bandwidth_gbps;
    (tx_time, tx_time + latency)
}

// TODO change this API, connect(a, b) function, connectable just has functions for giving and
// getting queues.
/// A standard interface for connecting devices of all types
pub trait Connectable {
    /// The unique ID of this connectable
    fn id(&self) -> usize;

    /// Connect these two routers together, public facing
    fn connect(&mut self, other: impl Connectable);

    /// Called by connect to establish the connection the other way
    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent>;
}

/// Takes care of properly building the simulation object, running it, and reporting to the user
pub fn run_config(config: SimConfig, n_cpus: usize) -> Result<(), Box<dyn Error>> {
    eprintln!("Setup...");

    eprintln!("  Creating network... ");
    let (net, n_hosts) = match config.topology {
        Topology::CLOS(u, d) => build_clos(u, d),
        Topology::FullyConnected(k) => build_fc(k, k - 1),
    };
    let n_links: u64 = (&net).iter().map(|(_, v)| v.len() as u64).sum();
    eprintln!(
        "    {} devices, {} hosts, {} links",
        net.len(),
        n_hosts,
        n_links
    );

    let mut world = World::new_from_network(net, &config, n_hosts);

    // Flows
    let mut flows = Vec::new();
    let flow_rdr = ReaderBuilder::new()
        .has_headers(false)
        .delimiter(b' ')
        .from_path(config.flow_file)
        .expect("File open failed");

    for try_line in flow_rdr.into_records() {
        let line = try_line?;

        // source is 0-indexed...
        let src = line[0].parse::<usize>()? + 1;
        let dst = line[1].parse::<usize>()? + 1;
        let size_byte = line[2].parse::<u64>()?;
        let time = line[3].parse::<u64>()?;

        if time > config.time_limit {
            break;
        }

        let flow: FlowDesc = (src, dst, size_byte);
        flows.push((time, flow));
    }
    /*/
    let mut flow_id = 0;
    for src_id in 1..(n_hosts + 1) {
        for dst_id in 1..(n_hosts + 1) {
            // skip self flows...
            if src_id == dst_id {
                continue;
            }

            let f = Flow::new(flow_id, src_id, dst_id, 100000000);
            flow_id += 1;
            flows.push(((flow_id * 1_000) as Time, f));
        }
    }
    */
    world.add_flows(flows);

    eprintln!("Running on {} cores...", n_cpus);
    let start = Instant::now();
    let counts = world.start(n_cpus, config.time_limit);
    let duration = start.elapsed();
    eprintln!("  ok");

    let n_actors = counts.len();
    let n_cpus = std::cmp::min(n_cpus, n_actors);

    // stats...
    let sum_count = counts.iter().sum::<ActorResult>();
    let ns_per_count: f64 = if sum_count > 0 {
        1000. * duration.as_nanos() as f64 / sum_count as f64
    } else {
        0.
    };

    // time_limit/1e9 is in seconds which is how much we simulated
    // divide by the time it took us -> simulation bandwidth
    let gbps =
        (n_links * config.bandwidth_gbps * config.time_limit) as f64 / 1e9 / duration.as_secs_f64();

    eprintln!(
        "= {} in {:.3}s. {} actors ({} hosts) for {:.3}s on {} cores",
        sum_count,
        duration.as_secs_f32(),
        n_actors,
        n_hosts,
        config.time_limit as f64 / 1e9,
        n_cpus,
    );
    eprintln!(
        "  {:.3}M count/sec, {:.3}M /actors, {:.3}M /cpu",
        (1e6 / ns_per_count as f64),
        (1e6 / (ns_per_count * n_actors as f64)),
        (1e6 / (ns_per_count * n_cpus as f64)),
    );
    eprintln!(
        "  {:.1} ns/count, {:.1} ns/actor, {:.1} ns/cpu",
        ns_per_count / 1000. as f64,
        ns_per_count * n_actors as f64 / 1000.,
        ns_per_count * n_cpus as f64 / 1000.
    );
    eprintln!(
        "  {:.3} gbps, {:.3} gbps/actor ({} links total)",
        gbps,
        (gbps / n_actors as f64),
        n_links,
    );

    eprintln!("done");
    Ok(())
}

/// Main simulation object.
///
/// This is where the core of the simualtion setup should happen. Notably it has the important
/// tasks of connection actors together, and to give "external" events to these actors.
///
/// Setting up and running the simulation are done in two phases. This feels like good design, but
/// it is not clear to me why.
#[derive(Debug)]
pub struct World {
    /// The actors themselves
    routers: Vec<Router>,
    servers: Vec<Server>,

    /// Communication channels from us (the world) to the actors
    chans: HashMap<usize, Producer<ModelEvent>>,
}

impl World {
    /// Builds a world based on the network
    pub fn new_from_network(network: Network, config: &SimConfig, n_hosts: usize) -> World {
        let mut server_builders: Vec<ServerBuilder> = Vec::new();
        let mut router_builders: Vec<RouterBuilder> = Vec::new();

        // Host builders, they don't connect to anything else
        for id in 1..n_hosts + 1 {
            server_builders.push(
                ServerBuilder::new(id)
                    .latency_ns(config.latency_ns)
                    .bandwidth_gbps(config.bandwidth_gbps),
            );
        }

        // Router builders, we can connect those we know about
        for id in n_hosts + 1..network.len() + 1 {
            let mut rb = RouterBuilder::new(id)
                .latency_ns(config.latency_ns)
                .bandwidth_gbps(config.bandwidth_gbps);
            for &n in &network[&id] {
                // skip those who are not connected yet...
                if n >= id {
                    continue;
                }

                if n <= n_hosts {
                    server_builders.get_mut(n - 1).unwrap().connect(&mut rb);
                } else {
                    router_builders
                        .get_mut(n - n_hosts - 1)
                        .unwrap()
                        .connect(&mut rb);
                }
            }
            router_builders.push(rb);
        }

        // Routing ---------------------------------------------
        eprintln!("  Routing...");
        router_builders
            .iter_mut()
            .map(|r| {
                let routes = route_all(&network, r.id);
                r.install_routes(routes);
            })
            .for_each(drop);

        // Instatiate everyone world
        let mut chans = HashMap::new();
        eprintln!("  Building {} servers...", server_builders.len());
        let mut servers = vec![];
        for mut b in server_builders {
            chans.insert(b.id, b.connect_world());
            servers.push(b.build());
        }

        eprintln!("  Building {} routers...", router_builders.len());
        let mut routers = vec![];
        for mut rb in router_builders {
            chans.insert(rb.id, rb.connect_world());
            routers.push(rb.build());
        }

        World {
            servers,
            routers,
            chans,
        }
    }

    /// Adds specified flows to the current network
    pub fn add_flows(&mut self, flows: Vec<(u64, FlowDesc)>) {
        eprintln!("  Init {} flows...", flows.len());
        for (time, f) in flows {
            self.chans[&f.0]
                .push(Event {
                    src: 0,
                    time,
                    event_type: EventType::ModelEvent(NetworkEvent::Flow(f)),
                })
                .unwrap();
        }
    }

    /// Runs this `World`'s simulation up to time `done`.
    ///
    /// This will spawn a thread per actor and wait for all of them to end.
    pub fn start(mut self, num_cpus: usize, done: u64) -> Vec<u64> {
        // csv header, cheating but that's okay here...
        println!("src,dst,start,end,size_byte,fct_ns");

        // Tell everyone when the end is
        for (_, c) in self.chans.iter_mut() {
            c.push(Event {
                time: done,
                src: 0,
                event_type: EventType::Close,
            })
            .unwrap();
        }

        // Workers
        let mut actors: Vec<Box<dyn Advancer<u64, u64> + Send>> = Vec::new();
        for s in self.servers {
            actors.push(Box::new(s));
        }
        for r in self.routers {
            actors.push(Box::new(r));
        }

        start(num_cpus, actors)
    }
}
