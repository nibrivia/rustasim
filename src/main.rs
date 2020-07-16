use rustasim::network;
//use rustasim::phold;

fn main() {
    let n_cpus = num_cpus::get() - 1;
    //phold::run(7 * 8 * 4, 0_100_000, n_cpus);

    //                      s  ms  us  ns
    //let time_limit: u64 = 001_000_000_000;
    #[allow(clippy::zero_prefixed_literal)]
    let time_limit: u64 = 000_011_111_000;
    let n_racks = 5;
    network::build_network(n_racks, time_limit, n_cpus);
}
