//! Generates the man page and shell completions from the same `Cli` definition used for parsing.

use clap::CommandFactory;
use clap_complete::Shell;
use std::env;
use std::fs;
use std::path::PathBuf;

include!("src/cli.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/cli.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
    let man_dir = out_dir.join("man");
    fs::create_dir_all(&man_dir).expect("failed to create man page output directory");

    let mut buffer = Vec::new();
    clap_mangen::Man::new(Cli::command())
        .render(&mut buffer)
        .expect("failed to render review-buddy.1 man page");
    fs::write(man_dir.join("review-buddy.1"), buffer).expect("failed to write review-buddy.1");

    let completions_dir = out_dir.join("completions");
    fs::create_dir_all(&completions_dir).expect("failed to create completions output directory");
    for (shell, file) in [
        (Shell::Bash, "review-buddy.bash"),
        (Shell::Zsh, "_review-buddy"),
        (Shell::Fish, "review-buddy.fish"),
        (Shell::Elvish, "review-buddy.elv"),
        (Shell::PowerShell, "_review-buddy.ps1"),
    ] {
        let mut buffer = Vec::new();
        clap_complete::generate(shell, &mut Cli::command(), "review-buddy", &mut buffer);
        fs::write(completions_dir.join(file), buffer).expect("failed to write completion script");
    }
}
