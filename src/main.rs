#![allow(unused, dead_code, unused_variables, unused_imports)]

use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process;

// read a file
fn folder_exists(path: &str) -> bool {
    let p = Path::new(path);
    p.exists() && p.is_dir()
}

fn gather_files(path: &str) -> Result<Vec<PathBuf>, io::Error> {
    let files = fs::read_dir(path)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_file() {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    Ok(files)
}

fn main() -> anyhow::Result<()> {
    println!("Hello, world!");
    let files = gather_files(".in")?;
    println!("{:?}", files);
    Ok(())
}
