use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

static OUT_DIR: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from(env::var("OUT_DIR").unwrap()));
static LIBNTIRPC_DIR: LazyLock<PathBuf> = LazyLock::new(|| OUT_DIR.join("ntirpc"));
static LIBNTIRPC_BUILD_DIR: LazyLock<PathBuf> = LazyLock::new(|| LIBNTIRPC_DIR.join("build"));
static LIBNTIRPC_INSTALL_DIR: LazyLock<PathBuf> = LazyLock::new(|| LIBNTIRPC_DIR.join("install"));

fn run<P: AsRef<Path>>(mut cmd: Command, path: P) {
    let dir = OUT_DIR.join(path.as_ref());
    println!("Running {:?} in {:?}", cmd, dir);
    cmd.current_dir(dir).status().unwrap();
}

fn download_and_extract() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("git clone https://github.com/nfs-ganesha/ntirpc.git");
    run(cmd, "");
}

fn configure() {
    let mut cmd = Command::new("mkdir");
    cmd.arg(&*LIBNTIRPC_BUILD_DIR);
    run(cmd, "");
    let mut cmd = Command::new("cmake");
    cmd.arg("-Wno-dev"); // supress developer warnings
    cmd.arg("-DUSE_LTTNG=Off");
    cmd.arg("-DCMAKE_BUILD_TYPE=RelWithDebInfo");
    cmd.arg(format!(
        "-DCMAKE_INSTALL_PREFIX={}",
        LIBNTIRPC_INSTALL_DIR.display()
    ));
    cmd.arg(&*LIBNTIRPC_DIR);
    run(cmd, &*LIBNTIRPC_BUILD_DIR);
}

fn make() {
    let cmd = Command::new("make");
    run(cmd, &*LIBNTIRPC_BUILD_DIR);
}

fn install() {
    let mut cmd = Command::new("make");
    cmd.arg("install");
    run(cmd, &*LIBNTIRPC_BUILD_DIR);
}

fn main() {
    if !LIBNTIRPC_DIR.exists() {
        download_and_extract();
    }

    if !LIBNTIRPC_INSTALL_DIR.exists() {
        configure();
    }
    make();
    install();

    println!(
        "cargo:rustc-link-search=native={}/lib",
        LIBNTIRPC_INSTALL_DIR.display()
    );
    println!("cargo:rustc-link-lib=dylib=ntirpc");

    bindgen::Builder::default()
        .header(format!(
            "{}/include/ntirpc/rpc/rpc.h",
            LIBNTIRPC_INSTALL_DIR.display()
        ))
        .clang_arg(format!(
            "-I{}/include/ntirpc",
            LIBNTIRPC_INSTALL_DIR.display()
        ))
        .clang_arg(format!("-I{}", LIBNTIRPC_BUILD_DIR.display()))
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
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(OUT_DIR.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
