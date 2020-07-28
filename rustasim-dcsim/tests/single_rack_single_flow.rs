use rustasim_dcsim::{connect, ActorResult, Network, SimConfig, Topology, World};

#[test]
fn single_flow() {
    #[allow(clippy::zero_prefixed_literal)]
    let time_limit: u64 = 16_020; // exact fct is 16_016

    let config = SimConfig {
        time_limit,                            // bogus
        topology: Topology::FullyConnected(1), // bogus
        flow_file: "".to_string(),             // bogus

        bandwidth_gbps: 10,
        latency_ns: 1_000,
    };

    // build simple network
    let mut net = Network::new();
    let n_hosts = 2;
    net.insert(1, vec![]);
    net.insert(2, vec![]);
    net.insert(3, vec![]);
    connect(&mut net, 1, 3);
    connect(&mut net, 2, 3);

    let mut world = World::new_from_network(net, &config, n_hosts);

    // single flow
    let mut flows = Vec::new();
    flows.push((0, (1, 2, 15_000)));
    world.add_flows(flows);

    let counts = world.start(1, config.time_limit);

    let sum_count = counts.iter().sum::<ActorResult>();
    println!("{:?}", counts);
    assert_eq!(counts.len(), 3);
    assert_eq!(sum_count, 1 + 10 + 20 + 10);
}
