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

pub struct World {
    /// The actors themselves
    racks: Vec<Router>,

    /// Communication channels from us (the world) to the actors
    chans: Vec<Producer<Event>>,
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
        let mut racks = Vec::new();
        for id in 1..n_racks + 1 {
            let mut r = Router::new(id);
            for id2 in 1..id {
                r.connect(racks.get_mut(id2 - 1).unwrap());
            }
            racks.push(r);
        }

        // flows
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
                        event_type: EventType::Packet(packet),
                    });
                }
                let dst_rack = racks.get_mut(dst - 1).unwrap();
                dst_rack.init_queue(src, packets);
            }
        }

        // TODO backbone switches

        // conect world
        let mut chans = Vec::new();
        for r in &mut racks {
            chans.push(r.connect_world());
        }

        // reuturn world
        World { racks, chans }
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

        // Get the results
        let mut counts = Vec::new();
        for h in handles {
            let c = h.join().unwrap();
            counts.push(c);
        }

        counts
    }
}
