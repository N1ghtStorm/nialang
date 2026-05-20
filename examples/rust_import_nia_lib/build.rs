use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("examples crate is two levels below repo root");
    let nia_src = manifest_dir.join("nia_lib.nia");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let lib_path = out_dir.join(dynamic_lib_filename("nia_sample"));

    println!("cargo:rerun-if-changed={}", nia_src.display());
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("src/driver/pipeline.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("src/backend/codegen/mod.rs").display()
    );

    let status = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(repo_root.join("Cargo.toml"))
        .arg("--")
        .arg(&nia_src)
        .arg("--lib")
        .arg("-o")
        .arg(&lib_path)
        .status()
        .expect("failed to run nialang compiler");
    if !status.success() {
        panic!("nialang failed to build {}", lib_path.display());
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=dylib=nia_sample");
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", out_dir.display());
    }
}

fn dynamic_lib_filename(stem: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else if cfg!(windows) {
        format!("{stem}.dll")
    } else {
        format!("lib{stem}.so")
    }
}
