//! Parallel datacenter network simulator
//!
//! Throughout this crate there is a user-backend relationship between the [simulation
//! engine](synchronizer/index.html) and the model. In general, the engine should be agnostic to
//! the type of model being run, and should probably eventually be pulled out into its own crate.

use crossbeam_queue::spsc::*;
use std::thread;

pub mod engine;
pub mod network;

use crate::engine::*;
use crate::network::nic::*;
use crate::network::routing::Network;
use crate::network::tcp::*;
use crate::network::ModelEvent;
use crate::network::NetworkEvent;

pub struct World {
    /// The actors themselves
    racks: Vec<Router>,
    servers: Vec<Server>,

    /// Communication channels from us (the world) to the actors
    chans: Vec<Producer<ModelEvent>>,
}

/// Main simulation object.
///
/// This is where the core of the simualtion setup should happen. Notably it has the important
/// tasks of connection actors together, and to give "external" events to these actors.
///
/// Setthing up and running the simulation are done in two phases. This feels like good design, but
/// it is not clear to me why.
impl World {
    /// Sets up a world ready for simulation
    pub fn new(n_racks: usize) -> World {
        // TODO pass as argument
        let servers_per_rack = n_racks;

        // Use to keep track of ID numbers and make them continuous
        let mut next_id = 1;
        let mut network = Network::new();

        // TODO backbone switches

        // Racks -----------------------------------------------
        let mut racks: Vec<Router> = Vec::new();

        for _ in 0..n_racks {
            let mut r = Router::new(next_id);
            network.insert(next_id, vec![]);

            // connect up with other racks
            for rack2 in racks.iter_mut() {
                // update our network matrix
                network.get_mut(&next_id).unwrap().push(rack2.id());
                network.get_mut(&rack2.id()).unwrap().push(next_id);

                // connect the devices
                (&mut r).connect(rack2);
            }

            next_id += 1;
            racks.push(r);
        }

        // Servers ---------------------------------------------
        let mut servers = Vec::new();

        for rack_ix in 0..n_racks {
            for _ in 0..servers_per_rack {
                let mut s = Server::new(next_id);
                network.insert(next_id, vec![]);

                // get the parent rack (needs to be done each time, ref is consumed by connect)
                let rack = racks.get_mut(rack_ix).unwrap();

                // update network matrix
                network.get_mut(&next_id).unwrap().push(rack.id());
                network.get_mut(&rack.id()).unwrap().push(next_id);

                // connect devices
                (&mut s).connect(rack);

                // push and consume
                next_id += 1;
                servers.push(s);
            }
        }

        // World -----------------------------------------------
        let mut chans = Vec::new();
        for s in &mut servers {
            chans.push(s.connect_world());
        }
        for r in &mut racks {
            chans.push(r.connect_world());
        }

        // Flows -----------------------------------------------
        let f = Flow::new(99, 100, 40000);
        chans[0]
            .push(Event {
                src: 0,
                time: 0,
                event_type: EventType::ModelEvent(NetworkEvent::Flow(f)),
            })
            .unwrap();

        for src in 1..n_racks + 1 {
            for dst in 1..n_racks + 1 {
                // skip self->self
                if src == dst {
                    continue;
                }
                //break;

                // create flow
                let f = Flow::new(src, dst, 10);

                // schedule on source
                let mut packets = Vec::new();
                for packet in f {
                    packets.push(Event {
                        src: dst,
                        time: 0,
                        event_type: EventType::ModelEvent(NetworkEvent::Packet(packet)),
                    });
                }
                let dst_rack = racks.get_mut(dst - 1).unwrap();
                dst_rack.init_queue(src, packets);
            }
        }

        // return world
        World {
            racks,
            servers,
            chans,
        }
    }

    /// Runs this `World`'s simulation up to time `done`.
    ///
    /// This will spawn a thread per actor and wait for all of them to end.
    pub fn start(mut self, done: u64) -> Vec<u64> {
        // Tell everyone when the end is
        for c in self.chans.iter_mut() {
            c.push(Event {
                time: done,
                src: 0,
                event_type: EventType::Close,
            })
            .unwrap();
        }

        // Start each rack in its own thread
        let mut handles = Vec::new();
        for r in self.racks {
            handles.push(thread::spawn(move || r.start()));
        }
        for s in self.servers {
            handles.push(thread::spawn(move || s.start()));
        }

        // Get the results
        let mut counts = Vec::new();
        for h in handles {
            let c = h.join().unwrap();
            counts.push(c);
        }

        counts
    }
}
