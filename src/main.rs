use num_cpus;
use rustasim::phold;

fn main() {
    let n_cpus = num_cpus::get() - 1;
    phold::run(4 * 8, 100_000, n_cpus);
}
