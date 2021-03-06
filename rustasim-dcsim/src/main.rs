use docopt::Docopt;
use rustasim_dcsim::{run_config, SimConfig, Topology};
use serde::Deserialize;

const USAGE: &str = "
Rustasim datacenter simulator.

Usage:
    rustasim-dcsim [--limit TIME] (--flows FILE | --load LOAD) ((--clos-up UP --clos-down DOWN) | --fc <N_RACKS>) [--latency LATENCY --bandwidth BANDWIDTH]
    rustasim-dcsim --help

Options:
    -h, --help               Show this message.

    -l, --limit TIME         Simulation end in seconds [default: 1.0].
    --latency LATENCY        Inter-device latency, in ns [default: 500].
    --bandwidth BANDWIDTH    Inter-device bandwidth, in gigabits/sec (note: bits, NOT bytes) [default: 10].

    -f, --flows FILE         Flow file, following the htsim format.
    --load LOAD              (unused) Load between 0 and 1, hosts will average LOAD*BANDWIDTH network demand

    --clos-up UP             Use 3:1 clos topology with UP uplinks
    --clos-down DOWN         Use 3:1 clos topology with DOWN downlinks.
    --fc <N_RACKS>           Use fully-connected topology with given number of racks.
";

#[derive(Debug, Deserialize)]
struct Args {
    // ui
    flag_help: bool,

    // timing and networ properties
    flag_limit: f64,
    flag_latency: u64,
    flag_bandwidth: u64,

    // flows
    flag_flows: Option<String>,
    flag_load: Option<f64>,

    // topology
    flag_clos_up: Option<usize>,
    flag_clos_down: Option<usize>,
    flag_fc: Option<usize>,
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());

    // print help and exit
    if args.flag_help {
        print!("{}", USAGE);
        return;
    }

    let topology = if let Some(n_racks) = args.flag_fc {
        Topology::FullyConnected(n_racks)
    } else if args.flag_clos_up.is_some() && args.flag_clos_down.is_some() {
        let u = args.flag_clos_up.unwrap();
        let d = args.flag_clos_down.unwrap();
        if (u + d) % 2 != 0 {
            eprintln!("For a CLOS topology to be valid, k (u+d) must be even! You gave CLOS({}, {}), k: {}\nexit", u, d, u+d);
            std::process::exit(1);
        }
        Topology::CLOS(u, d)
    } else {
        eprint!("FAIL: Couldn't parse topology...\n\n{}", USAGE);
        std::process::exit(1);
    };

    if args.flag_load.is_some() {
        unimplemented!();
    }

    #[allow(clippy::zero_prefixed_literal)]
    let time_limit: u64 = (args.flag_limit * 1e9) as u64;

    let config = SimConfig {
        time_limit,
        topology,
        flow_file: args.flag_flows.unwrap(),

        bandwidth_gbps: args.flag_bandwidth,

        latency_ns: args.flag_latency,
    };

    let n_cpus = num_cpus::get() - 1;
    if let Err(e) = run_config(config, n_cpus) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
