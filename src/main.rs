use num_cpus;
//use rustasim::network;
use rustasim::phold;

fn main() {
    let n_cpus = num_cpus::get() - 1;
    phold::run(7 * 8, 1_000_000, n_cpus);

    //                      s  ms  us  ns
    //let time_limit: u64 = 001_000_000_000;
    //let time_limit: u64 = 000_111_111_000;
    //let n_racks = 25;
    //network::build_network(n_racks, time_limit, n_cpus);
}
