use std::{fs, path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/package.json");

    println!("Building frontend...");
    let status = Command::new("bun")
        .args(["run", "build"])
        .current_dir("frontend")
        .status()
        .expect("Failed to build frontend");

    if !status.success() {
        panic!("Frontend build failed");
    }
}
