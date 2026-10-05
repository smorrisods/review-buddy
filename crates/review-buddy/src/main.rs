mod cli;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let _cli = cli::Cli::parse();
    println!(
        "review-buddy {}: the TUI is not built yet.",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}
