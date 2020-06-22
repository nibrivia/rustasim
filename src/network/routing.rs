//! Self contained module for initial routing utilities
//!
//! A lot of research is going into dynamic routing, this doesn't take place here, just the
//! standard initial map. This may change in the future, I'm not sure...

use std::collections::HashMap;

/// Given a list of edges, find the routing table for the given `id`
///
/// This assumes all the edges have the same weight and breaks ties arbitrarely. Eventually this
/// probably should return the cost of the path and alternatives for equal-cost multi-path
pub fn route_id(network: HashMap<usize, Vec<usize>>, source_id: usize) -> HashMap<usize, usize> {
    let mut route = HashMap::new();
    let mut queue = vec![(source_id, 0)];

    while !queue.is_empty() {
        // this is the new candidate and its cost
        let (id, cost) = queue.pop().unwrap();

        // If we've already visited this, under the equal cost assumption it means that it's
        // already done
        if route.contains_key(&id) {
            continue;
        }

        route.insert(id, cost);

        for neighbour_id in &network[&id] {
            // necessary to avoid loops
            if route.contains_key(neighbour_id) {
                continue;
            }

            // add neighbour to the queue
            queue.push((*neighbour_id, cost + 1));
        }
    }


    route
}
