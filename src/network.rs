//! Datacenter network model

use crossbeam_queue::spsc::Producer;

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
