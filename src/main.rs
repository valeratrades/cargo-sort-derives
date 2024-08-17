mod ext;
mod grep;
mod sort;

use clap::{Args, Parser};
use grep::grep;
use sort::process_file;

#[derive(Debug, Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cli {
    SortDerives(SortDerivesArgs),
}

#[derive(Debug, Args)]
#[command(version, about, long_about = None)]
struct SortDerivesArgs {
    /// Check if the derive attributes are sorted
    #[clap(long)]
    check: bool,
}

fn main() {
    let Cli::SortDerives(args) = Cli::parse();

    for (file_path, line_numbers) in grep().unwrap() {
        process_file(&file_path, line_numbers, args.check).unwrap();
    }
}
