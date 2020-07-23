use rustasim_dcsim::{run_config, SimConfig, Topology};

fn main() {
    // TODO read from command line

    //                      s  ms  us  ns
    //let time_limit: u64 = 001_000_000_000;
    #[allow(clippy::zero_prefixed_literal)]
    let time_limit: u64 = 000_051_111_000;

    let config = SimConfig {
        time_limit,
        topology: Topology::CLOS(3, 9),
        flow_file: "/home/nibr/opera-sim/Figure7_datamining/3to1_clos/traffic_gen/flows_25percLoad_10sec_648hosts_3to1.htsim".to_string(),
    };

    let n_cpus = num_cpus::get() - 1;
    if let Err(e) = run_config(config, n_cpus) {
        println!("Error: {}", e);
        std::process::exit(1);
    }
}
