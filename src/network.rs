//! Datacenter network model

pub mod nic;
pub mod tcp;

/// Datacenter network model events
pub enum NetworkEvent {
    /// Flow start
    Flow(tcp::Flow),

    /// Packet arrival
    Packet(tcp::Packet),
}

pub type ModelEvent = crate::engine::Event<NetworkEvent>;
