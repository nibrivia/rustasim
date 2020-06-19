//! Parallel datacenter network simulator
//!
//! Throughout this crate there is a user-backend relationship between the [simulation
//! engine](synchronizer/index.html) and the model. In general, the engine should be agnostic to
//! the type of model being run, and should probably eventually be pulled out into its own crate.

use crossbeam::queue::spsc::*;
use std::thread;

pub mod engine;
pub mod network;

use crate::engine::*;
use crate::network::nic::*;
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
        // Create the racks and connect them all up
        let mut racks: Vec<Router> = Vec::new();

        for id in 1..n_racks + 1 {
            let mut r = Router::new(id);
            for id2 in 1..id {
                let rack2 = racks.get_mut(id2 - 1).unwrap();
                (&mut r).connect(rack2);
            }
            racks.push(r);
        }

        let mut servers = Vec::new();

        // source under rack 0
        let mut s = Server::new(99);
        (&mut s).connect(racks.get_mut(0).unwrap());
        servers.push(s);

        // dest under rack 2
        let mut d = Server::new(100);
        (&mut d).connect(racks.get_mut(2).unwrap());
        servers.push(d);

        // conect world
        let mut chans = Vec::new();
        for s in &mut servers {
            chans.push(s.connect_world());
        }
        for r in &mut racks {
            chans.push(r.connect_world());
        }

        // flows
        let f = Flow::new(99, 100, 40);
        chans[0]
            .push(Event {
                src: 0,
                time: 0,
                event_type: EventType::ModelEvent(NetworkEvent::Flow(f)),
            })
            .unwrap();

        /*
        for src in 1..n_racks + 1 {
            for dst in 1..n_racks + 1 {
                // skip self->self
                if src == dst {
                    continue;
                }
                //break;

                // create flow
                let f = Flow::new(src, dst, 40);

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
        */

        // TODO backbone switches

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
