use std::process;

use minecraft_k8s::cli;

fn main() {
    process::exit(cli::entry());
}
