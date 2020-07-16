//! Datacenter network model

use crate::engine::*;
use crate::network::router::{Router, RouterBuilder};
use crate::network::routing::{route_id, Network};
use crate::network::server::{Server, ServerBuilder};
use crate::network::tcp::*;
use crate::start;
use crate::worker::Advancer;
use crossbeam_queue::spsc::Producer;
use std::collections::HashMap;
use std::time::Instant;

mod router;
pub mod routing;
mod server;
mod tcp;

const Q_SIZE: usize = 1 << 13;

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

type ModelEvent = crate::engine::Event<u64, NetworkEvent>;

/// Device types
#[derive(Debug)]
pub enum Device {
    /// Router device type
    Router,

    /// Server device type
    Server,
}

// TODO change this API, connect(a, b) function, connectable just has functions for giving and
// getting queues.
/// A standard interface for connecting devices of all types
pub trait Connectable {
    /// The unique ID of this connectable
    fn id(&self) -> usize;

    /// Whether it is a Router or a Server
    fn flavor(&self) -> Device;

    /// Connect these two routers together, public facing
    fn connect(&mut self, other: impl Connectable);

    /// Called by connect to establish the connection the other way
    fn back_connect(
        &mut self,
        other: impl Connectable,
        tx_queue: Producer<ModelEvent>,
    ) -> Producer<ModelEvent>;
}

/// Builds and runs a network with the given parameters
///
/// TODO more...
pub fn build_network(n_racks: usize, time_limit: u64, n_cpus: usize) {
    // TODO pass in time_limit, n_threads as arguments

    //let time_limit: u64 = 1_000_000_000;

    println!("Setup...");
    //let (net, n_hosts) = routing::build_fc(5, 4);
    let (net, n_hosts) = routing::build_clos(2, 2);
    let world = World::new_from_network(net, n_hosts);

    println!("Run...");
    let start = Instant::now();
    let counts = world.start(n_cpus, time_limit);
    let duration = start.elapsed();

    let n_actors = counts.len();
    let n_cpus = std::cmp::min(n_cpus, n_actors);

    // TODO make general
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
        "= {} in {:.3}s. {} actors, {} hosts, {} cores",
        sum_count,
        duration.as_secs_f32(),
        n_actors,
        n_hosts,
        n_cpus,
    );
    println!(
        "  {:.3}M count/sec, {:.3}M /actors, {:.3}M /cpu",
        (1e6 / ns_per_count as f64),
        (1e6 / (ns_per_count * n_actors as f64)),
        (1e6 / (ns_per_count * n_cpus as f64)),
    );
    println!(
        "  {:.1} ns/count, {:.1} ns/actor, {:.1} ns/cpu",
        ns_per_count / 1000. as f64,
        ns_per_count * n_actors as f64 / 1000.,
        ns_per_count * n_cpus as f64 / 1000.
    );
    println!(
        "  {:.3} gbps, {:.3} gbps/actor ({} links total)",
        gbps,
        (gbps / n_actors as f64),
        n_links
    );

    println!("done");
}

#[derive(Debug)]
struct World {
    /// The actors themselves
    routers: Vec<Router>,
    servers: Vec<Server>,

    /// Communication channels from us (the world) to the actors
    chans: HashMap<usize, Producer<ModelEvent>>,
}

/// Main simulation object.
///
/// This is where the core of the simualtion setup should happen. Notably it has the important
/// tasks of connection actors together, and to give "external" events to these actors.
///
/// Setthing up and running the simulation are done in two phases. This feels like good design, but
/// it is not clear to me why.
impl World {
    pub fn new_from_network(network: Network, n_hosts: usize) -> World {
        let mut server_builders: Vec<ServerBuilder> = Vec::new();
        let mut router_builders: Vec<RouterBuilder> = Vec::new();

        // Host builders, they don't connect to anything else
        for id in 1..n_hosts + 1 {
            server_builders.push(ServerBuilder::new(id));
        }

        // Router builders, we can connect those we know about
        for id in n_hosts + 1..network.len() + 1 {
            let mut rb = RouterBuilder::new(id);
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
        for r in router_builders.iter_mut() {
            let rack_id = r.id;

            let routes = route_id(&network, rack_id);
            r.install_routes(routes);
        }

        // Instatiate everyone world
        let mut chans = HashMap::new();
        let mut servers = vec![];
        for mut b in server_builders {
            chans.insert(b.id, b.connect_world());
            servers.push(b.build());
        }

        let mut routers = vec![];
        for mut rb in router_builders {
            chans.insert(rb.id, rb.connect_world());
            routers.push(rb.build());
        }

        // Flows -----------------------------------------------
        let mut flow_id = 0;
        for src in servers.iter() {
            for dst in servers.iter() {
                // skip self flows...
                if src.id == dst.id {
                    continue;
                }

                let f = Flow::new(flow_id, src.id, dst.id, 100000000);
                flow_id += 1;

                chans[&src.id]
                    .push(Event {
                        src: 0,
                        time: 0,
                        event_type: EventType::ModelEvent(NetworkEvent::Flow(f)),
                    })
                    .unwrap();
            }
        }

        World {
            servers,
            routers,
            chans,
        }
    }
    /// Sets up a world ready for simulation
    pub fn _new(n_racks: usize) -> World {
        // TODO pass as argument
        let servers_per_rack = n_racks - 1;

        // Use to keep track of ID numbers and make them continuous
        let mut next_id = 1;
        let mut network = Network::new(); // for routing

        // Racks -----------------------------------------------
        let mut rack_builders: Vec<RouterBuilder> = Vec::new();

        for _ in 0..n_racks {
            let mut r = RouterBuilder::new(next_id);
            network.insert(next_id, vec![]);

            // connect up with other racks
            for rack2 in rack_builders.iter_mut() {
                // update our network matrix
                network.get_mut(&next_id).unwrap().push(rack2.id());
                network.get_mut(&rack2.id()).unwrap().push(next_id);

                // connect the devices
                (&mut r).connect(rack2);
            }

            next_id += 1;
            rack_builders.push(r);
        }

        // Servers ---------------------------------------------
        let mut server_builders = Vec::new();

        for rack_ix in 0..n_racks {
            for _ in 0..servers_per_rack {
                let mut s = ServerBuilder::new(next_id);
                network.insert(next_id, vec![]);

                // get the parent rack (needs to be done each time, ref is consumed by connect)
                let rack = rack_builders.get_mut(rack_ix).unwrap();

                // update network matrix
                network.get_mut(&next_id).unwrap().push(rack.id());
                network.get_mut(&rack.id()).unwrap().push(next_id);

                // connect devices
                (&mut s).connect(rack);

                // push and consume
                next_id += 1;
                server_builders.push(s);
            }
        }

        // Routing ---------------------------------------------
        for r in rack_builders.iter_mut() {
            let rack_id = r.id();

            let routes = route_id(&network, rack_id);
            r.install_routes(routes);
        }

        // World -----------------------------------------------
        let mut chans = HashMap::new();
        let mut servers = vec![];
        for mut b in server_builders {
            chans.insert(b.id, b.connect_world());
            servers.push(b.build());
        }

        let mut routers = vec![];
        for mut rb in rack_builders {
            chans.insert(rb.id, rb.connect_world());
            routers.push(rb.build());
        }

        // Flows -----------------------------------------------
        let mut flow_id = 0;
        for src in servers.iter() {
            for dst in servers.iter() {
                // skip self flows...
                if src.id == dst.id {
                    continue;
                }

                let f = Flow::new(flow_id, src.id, dst.id, 100000000);
                flow_id += 1;

                chans[&src.id]
                    .push(Event {
                        src: 0,
                        time: 0,
                        event_type: EventType::ModelEvent(NetworkEvent::Flow(f)),
                    })
                    .unwrap();
            }
        }

        // return world
        World {
            routers,
            servers,
            chans,
        }
    }

    /// Runs this `World`'s simulation up to time `done`.
    ///
    /// This will spawn a thread per actor and wait for all of them to end.
    pub fn start(mut self, num_cpus: usize, done: u64) -> Vec<u64> {
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
