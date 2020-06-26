//! Datacenter network model

pub mod nic;
pub mod routing;
pub mod tcp;

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
