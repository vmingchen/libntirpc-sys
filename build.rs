use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::process::Command;

static OUT_DIR: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from(env::var("OUT_DIR").unwrap()) );
static LIBNTIRPC_DIR: LazyLock<PathBuf> = LazyLock::new(|| OUT_DIR.join("ntirpc"));
static LIBNTIRPC_OUTPUT_DIR: LazyLock<PathBuf> = LazyLock::new(|| OUT_DIR.join("ntirpc/output"));

fn run<P: AsRef<Path>>(mut cmd: Command, path: P) {
    let dir = OUT_DIR.join(path.as_ref());
    println!("Running {:?} in {:?}", cmd, dir);
    cmd.current_dir(dir).status().unwrap();
}

fn download_and_extract() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("git clone https://github.com/nfs-ganesha/ntirpc.git");
    run(cmd, "");
}

fn configure() {
    let mut cmd = Command::new("mkdir");
    cmd.arg(&*LIBNTIRPC_OUTPUT_DIR);
    run(cmd, "");
    let mut cmd = Command::new("cmake");
    cmd.arg("-DCMAKE_BUILD_TYPE=RelWithDebInfo").arg("..");
    run(cmd, &*LIBNTIRPC_OUTPUT_DIR);
}

fn make() {
    let cmd = Command::new("make");
    run(cmd, &*LIBNTIRPC_OUTPUT_DIR);
}

fn install() {
    let mut cmd = Command::new("make");
    cmd.arg("install");
    run(cmd, &*LIBNTIRPC_OUTPUT_DIR);
}

fn main() {
    if !LIBNTIRPC_DIR.exists() {
        download_and_extract();
    }

    if !LIBNTIRPC_OUTPUT_DIR.exists() {
        configure();
    }
    make();
    install();

    println!(
        "cargo:rustc-link-search=native={}/lib",
        LIBNTIRPC_OUTPUT_DIR.display()
    );
    println!("cargo:rustc-link-lib=static=ntirpc");

    bindgen::Builder::default()
        .header(LIBNTIRPC_DIR.join("ntirpc/rpc/rpc.h").display().to_string())
        .blocklist_type("rpcblist")
        // Following are unsupported because of usage u128
        .blocklist_function("xdr_quadruple")
        .blocklist_function("strtold")
        .blocklist_type("_Float64x")
        .blocklist_function("qecvt_r")
        .blocklist_function("qfcvt_r")
        .blocklist_function("qecvt")
        .blocklist_function("qfcvt")
        .blocklist_function("qgcvt")
        .clang_arg(format!("-I{}/ntirpc", LIBNTIRPC_DIR.display()))
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(OUT_DIR.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
