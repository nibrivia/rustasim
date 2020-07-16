//! Self contained module for initial routing utilities
//!
//! A lot of research is going into dynamic routing, this doesn't take place here, just the
//! standard initial map. This may change in the future, I'm not sure...

use std::collections::HashMap;

/// Convenient wrapper for the network description
pub type Network = HashMap<usize, Vec<usize>>;

/// Given `network` a map of nodes and their neighbours, find the routing table for the given `id`
///
/// This assumes all the edges have the same weight and breaks ties arbitrarely. Eventually this
/// probably should return the cost of the path and alternatives for equal-cost multi-path.
///
/// The route to self can return arbitrary values, but will exist. Do not rely on it being 0.
///
/// # Examples
/// ```
/// use std::collections::HashMap;
/// use rustasim::network::routing::route_id;
///
/// // +-------+
/// // |       |
/// // 1 - 2 - 3 - 4
/// let mut network = HashMap::new();
/// network.insert(1, vec![2, 3]);
/// network.insert(2, vec![1, 3]);
/// network.insert(3, vec![1, 2, 4]);
/// network.insert(4, vec![3]);
///
/// // route from 1
/// let route = route_id(&network, 1);
/// // assert_eq!(route[&1], 0);
/// assert_eq!(route[&2], 2);
/// assert_eq!(route[&3], 3);
/// assert_eq!(route[&4], 3);
///
/// // route from 2
/// let route = route_id(&network, 2);
/// assert_eq!(route[&1], 1);
/// // assert_eq!(route[&2], 0);
/// assert_eq!(route[&3], 3);
/// assert_eq!(route[&4], 3);
/// ```
pub fn route_id(network: &Network, source_id: usize) -> HashMap<usize, usize> {
    // temporary map from id -> (next_hop, cost)
    let mut route_cost = HashMap::new();
    route_cost.insert(source_id, (source_id, 0)); // self routing is weird...

    // initialize queeu with neighbours
    let mut queue = vec![];
    for neighb in &network[&source_id] {
        queue.push((*neighb, *neighb, 1));
    }

    while !queue.is_empty() {
        // this is the new candidate and its cost
        let (id, source, cost) = queue.pop().unwrap();

        // only keep going if the new cost is lower
        if let Some((_, cur_cost)) = route_cost.get(&id) {
            if *cur_cost < cost {
                continue;
            }
        }

        // Add the path to the current node
        route_cost.insert(id, (source, cost));

        // Add our neighbours to the queue
        for neighbour_id in &network[&id] {
            // add neighbour to the queue
            queue.push((*neighbour_id, source, cost + 1));
        }
    }

    // translate into a pure routing table, no more cost
    let mut route = HashMap::new();
    for (node, (hop, _)) in route_cost {
        route.insert(node, hop);
    }
    route
}

/// Bi-directionally connects the
pub fn connect(net: &mut Network, src: usize, dst: usize) {
    net.get_mut(&src).unwrap().push(dst);
    net.get_mut(&dst).unwrap().push(src);
}

/// Builds a 3:1 folded CLOS network of 648 hosts, like the Opera paper
///
/// See the [Opera
/// simulator](https://github.com/TritonNetworking/opera-sim/blob/master/src/clos/datacenter/fat_tree_topology_3to1_k12.cpp)
/// for more details.
///
/// At some point should probably become more general
pub fn build_clos(u: usize, d: usize) -> Network {
    let mut net = Network::new();

    let k = u + d;

    let n_pods = k; // this is the number of pods a single core can support

    let hosts_per_rack = d; // by definition of d
    let upper_per_pod = u; // by definition of u

    let racks_per_pod = k / 2; // because upods are evenly matched, each upod connected to all racks
    let n_cores = upper_per_pod * k / 2; // it takes all the upods of a pod to connect to all the cores

    let n_upper_pods = n_pods * upper_per_pod;
    let n_racks = n_pods * racks_per_pod;
    let n_hosts = n_racks * hosts_per_rack;

    let n_devices = n_hosts + n_racks + n_upper_pods + n_cores;

    let mut hosts: Vec<usize> = Vec::new();
    for id in 0..n_devices {
        hosts.push(id);
        net.insert(id, vec![]);
    }

    let mut racks = hosts.split_off(n_hosts);
    let mut upper_pods = racks.split_off(n_racks);
    let cores = upper_pods.split_off(n_upper_pods);

    // hosts <> racks, each host connected to 1 rack
    for (host_ix, &host_id) in hosts.iter().enumerate() {
        let rack_id = racks[host_ix / hosts_per_rack];
        connect(&mut net, host_id, rack_id);
        println!(
            "host {} #{} connecting to rack {} #{}",
            host_ix,
            host_id,
            host_ix / hosts_per_rack,
            rack_id,
        );
    }

    // racks <> upper pod, each rack connected to 3 upper pods
    for (rack_ix, &rack_id) in racks.iter().enumerate() {
        let pod_id = rack_ix / racks_per_pod;
        for upod_offset in 0..upper_per_pod {
            println!(
                "rack {} #{} in pod {} connected to upod {} #...",
                rack_ix, rack_id, pod_id, upod_offset
            );
            let upper_pod_id = upper_pods[pod_id * upper_per_pod + upod_offset];
            connect(&mut net, rack_id, upper_pod_id);
        }
    }

    // upper pod <> core, the first up connected to the first 6 cores
    for (upod_ix, &upod_id) in upper_pods.iter().enumerate() {
        let core_offset = k / 2 * (upod_ix % upper_per_pod);
        for core_ix in 0..(k / 2) {
            println!(
                "upod {} #{} connecting to core {} #...",
                upod_ix,
                upod_id,
                core_offset + core_ix,
            );
            let core_id = cores[core_offset + core_ix];
            connect(&mut net, upod_id, core_id);
        }
    }

    net
}

#[cfg(test)]
mod test {
    use crate::network::routing::*;
    use std::collections::HashMap;

    /// Just to check all network are bi-direectional
    fn basic_net_checks(network: &Network) {
        println!("Network: {:#?}", network);
        for (node, neighbs) in network {
            for n in neighbs {
                assert!(
                    network[n].contains(node),
                    "{}>{} only goes one way...",
                    node,
                    n,
                );
            }
        }

        // TODO assert distinct?
    }

    fn basic_route_checks(network: &Network, route: &HashMap<usize, usize>, source: usize) {
        // there should be a route entry for every element of the network, except self
        assert_eq!(
            network.len(),
            route.len(),
            "Route doesn't have the right number of entries\n Route: {:#?}\n Network: {:#?}",
            route,
            network,
        );

        for (dst, next_hop) in route.iter() {
            // self routing is a little weird. let it be 0
            if *dst == source {
                // assert_eq!(*next_hop, 0, "Self routing should give 0, not {}", next_hop);
                continue;
            }

            // the network better contain that destination
            assert!(
                network.contains_key(dst),
                "Destination {} isn't part of the network {:?}...",
                dst,
                network,
            );

            // the next_hop should be a neighbour of ours
            assert!(
                network[&source].contains(next_hop),
                "Neighbour {} isn't a neighbour {:?}...",
                next_hop,
                network[&source],
            );
        }
    }

    #[test]
    fn clos_k12_u3d9() {
        let net = build_clos(3, 9);
        let n_hosts = 648;
        basic_net_checks(&net);

        for (&node, neighbs) in &net {
            if node < n_hosts {
                assert!(
                    neighbs.len() == 1,
                    "Host {} connected to >1 racks: {:?}!",
                    node,
                    neighbs
                );
            } else {
                assert!(
                    neighbs.len() == 12,
                    "Host {} connected to != 12 racks: {:?}!",
                    node,
                    neighbs
                );
            }
        }

        //let route = route_id(&net, 1);
        //basic_route_checks(&net, &route, 1);
    }

    #[test]
    fn clos_k8_u2d6() {
        let net = build_clos(2, 6);
        let n_hosts = 192;
        basic_net_checks(&net);

        for (&node, neighbs) in &net {
            if node < n_hosts {
                assert!(
                    neighbs.len() == 1,
                    "Host {} connected to >1 racks: {:?}!",
                    node,
                    neighbs
                );
            } else {
                assert!(
                    neighbs.len() == 8,
                    "Host {} connected to != 8 racks: {:?}!",
                    node,
                    neighbs
                );
            }
        }

        //let route = route_id(&net, 1);
        //basic_route_checks(&net, &route, 1);
    }

    #[test]
    fn clos_k12_u6d18() {
        let net = build_clos(6, 18);
        let n_hosts = 5_184;
        basic_net_checks(&net);

        for (&node, neighbs) in &net {
            if node < n_hosts {
                assert!(
                    neighbs.len() == 1,
                    "Host {} connected to >1 racks: {:?}!",
                    node,
                    neighbs
                );
            } else {
                assert!(
                    neighbs.len() == 24,
                    "Host {} connected to != 24 racks: {:?}!",
                    node,
                    neighbs
                );
            }
        }

        //let route = route_id(&net, 1);
        //basic_route_checks(&net, &route, 1);
    }

    #[test]
    fn single_node() {
        let mut network = Network::new();
        network.insert(1, Vec::new());

        let route = route_id(&network, 1);

        // there should be a destination for every element of the network
        basic_route_checks(&network, &route, 1);
        // assert_eq!(route[&1], 0);
    }

    #[test]
    fn two_node() {
        let mut network = Network::new();
        network.insert(1, vec![2]);
        network.insert(2, vec![1]);

        // from 1
        let route = route_id(&network, 1);

        // there should be a destination for every element of the network
        basic_route_checks(&network, &route, 1);
        // assert_eq!(route[&1], 0);
        assert_eq!(route[&2], 2);

        // from 2
        let route = route_id(&network, 2);

        // there should be a destination for every element of the network
        basic_route_checks(&network, &route, 2);
        assert_eq!(route[&1], 1);
        // assert_eq!(route[&2], 0);
    }

    #[test]
    fn line() {
        // 1 - 2 - 3 - 4
        let mut network = Network::new();
        network.insert(1, vec![2]);
        network.insert(2, vec![1, 3]);
        network.insert(3, vec![2, 4]);
        network.insert(4, vec![3]);

        // from 1
        let route = route_id(&network, 1);

        // there should be a destination for every element of the network
        basic_route_checks(&network, &route, 1);
        // assert_eq!(route[&1], 0);
        assert_eq!(route[&2], 2);
        assert_eq!(route[&3], 2);
        assert_eq!(route[&4], 2);

        // from 2
        let route = route_id(&network, 2);

        // there should be a destination for every element of the network
        basic_route_checks(&network, &route, 2);
        assert_eq!(route[&1], 1);
        // assert_eq!(route[&2], 0);
        assert_eq!(route[&3], 3);
        assert_eq!(route[&4], 3);
    }

    #[test]
    fn shortcut() {
        // +-------+
        // |       |
        // 1 - 2 - 3 - 4
        let mut network = Network::new();
        network.insert(1, vec![2, 3]);
        network.insert(2, vec![1, 3]);
        network.insert(3, vec![1, 2, 4]);
        network.insert(4, vec![3]);
        basic_net_checks(&network);

        // from 1
        let route = route_id(&network, 1);
        println!("Route[1]: {:#?}", route);
        basic_route_checks(&network, &route, 1);
        // assert_eq!(route[&1], 0);
        assert_eq!(route[&2], 2);
        assert_eq!(route[&3], 3);
        assert_eq!(route[&4], 3);

        // from 2
        let route = route_id(&network, 2);
        println!("Route[2]: {:#?}", route);
        basic_route_checks(&network, &route, 2);
        assert_eq!(route[&1], 1);
        // assert_eq!(route[&2], 0);
        assert_eq!(route[&3], 3);
        assert_eq!(route[&4], 3);
    }

    #[test]
    fn fully_connected_5() {
        let mut network = Network::new();
        network.insert(1, vec![2, 3, 4, 5]);
        network.insert(2, vec![1, 3, 4, 5]);
        network.insert(3, vec![1, 2, 4, 5]);
        network.insert(4, vec![1, 2, 3, 5]);
        network.insert(5, vec![1, 2, 3, 4]);
        basic_net_checks(&network);

        // from 1
        let route = route_id(&network, 1);
        println!("Route[1]: {:#?}", route);
        basic_route_checks(&network, &route, 1);
        // assert_eq!(route[&1], 0);
        assert_eq!(route[&2], 2);
        assert_eq!(route[&3], 3);
        assert_eq!(route[&4], 4);
        assert_eq!(route[&5], 5);
    }

    #[test]
    fn small_racks() {
        // the racks are 10, 20, 30 and fully connected to each other
        // the servers are 11, 12, 13 for rack 1, 21, 22, 23, for rack 2, etc...
        let mut network = Network::new();

        // racks
        network.insert(10, vec![11, 12, 13, 20, 30]);
        network.insert(20, vec![21, 22, 23, 10, 30]);
        network.insert(30, vec![31, 32, 33, 10, 20]);

        // servers
        network.insert(11, vec![10]);
        network.insert(12, vec![10]);
        network.insert(13, vec![10]);

        network.insert(21, vec![20]);
        network.insert(22, vec![20]);
        network.insert(23, vec![20]);

        network.insert(31, vec![30]);
        network.insert(32, vec![30]);
        network.insert(33, vec![30]);

        // double-check network construction
        basic_net_checks(&network);

        // from a rack
        let route = route_id(&network, 10);
        println!("Route[10]: {:#?}", route);
        basic_route_checks(&network, &route, 10);
        // assert_eq!(route[&10], 0); // self
        assert_eq!(route[&11], 11); // my servers
        assert_eq!(route[&12], 12);
        assert_eq!(route[&13], 13);

        assert_eq!(route[&20], 20); // rack
        assert_eq!(route[&21], 20); // its servers
        assert_eq!(route[&22], 20);
        assert_eq!(route[&23], 20);

        assert_eq!(route[&30], 30); // rack
        assert_eq!(route[&31], 30);
        assert_eq!(route[&32], 30);
        assert_eq!(route[&33], 30);

        // from a server
        let route = route_id(&network, 32);
        println!("Route[32]: {:#?}", route);
        basic_route_checks(&network, &route, 32);
        assert_eq!(route[&10], 30); // rack
        assert_eq!(route[&11], 30); // its servers
        assert_eq!(route[&12], 30);
        assert_eq!(route[&13], 30);

        assert_eq!(route[&20], 30); // rack
        assert_eq!(route[&21], 30); // its servers
        assert_eq!(route[&22], 30);
        assert_eq!(route[&23], 30);

        assert_eq!(route[&30], 30);
        assert_eq!(route[&31], 30);
        // assert_eq!(route[&32], 0); // myself
        assert_eq!(route[&33], 30);
    }

    // TODO backbone switches?
}
