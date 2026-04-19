use clap::Parser;
use juballer_deck::cli;

fn main() {
    let args = cli::Cli::parse();
    if let Err(e) = cli::run(args) {
        eprintln!("juballer-deck: {e}");
        std::process::exit(1);
    }
}
