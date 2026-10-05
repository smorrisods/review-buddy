//! Generates the man page from the same `Cli` definition used for parsing.

use clap::CommandFactory;
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
}
