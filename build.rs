use std::{env, path::PathBuf, process::Command};

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).expect("create native source directory");
    for entry in std::fs::read_dir(source).expect("read packaged native source") {
        let entry = entry.expect("native source entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("native source type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert!(
                entry.file_type().expect("native source type").is_file(),
                "native source must contain only regular files and directories"
            );
            std::fs::copy(entry.path(), target).expect("copy native source");
        }
    }
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{command:?}: {error}"));
    assert!(status.success(), "native build command failed: {command:?}");
}

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:legacy_free_cb=0");
    println!("cargo:native_reply_verifier=1");
    println!("cargo:rerun-if-changed=vendor/ntirpc");
    println!("cargo:rerun-if-changed=src/auth_helpers.c");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rerun-if-env-changed=CMAKE_TOOLCHAIN_FILE");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
    if env::var_os("DOCS_RS").is_some() {
        std::fs::copy("src/bindings-docs.rs", out.join("bindings.rs")).expect("docs bindings");
        println!("cargo:rerun-if-changed=src/bindings-docs.rs");
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("linux"),
        "bundled ntirpc currently supports Linux only"
    );
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    let toolchain = env::var_os("CMAKE_TOOLCHAIN_FILE");
    assert!(
        target == host || toolchain.is_some(),
        "cross compilation requires CMAKE_TOOLCHAIN_FILE and target dependency packages"
    );
    let gss = env::var_os("CARGO_FEATURE_RPCSEC_GSS").is_some();
    let build = out.join("bundled-build");
    let source = out.join("native-src");
    // Upstream CMake generates libntirpc.spec in its source tree. Never let
    // that mutate a Cargo registry/package checkout or the maintained source.
    copy_tree(std::path::Path::new("vendor/ntirpc"), &source);
    let prefix = out.join("native-install");
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source)
        .arg("-B")
        .arg(&build)
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display()))
        .args([
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DUSE_MONITORING=OFF",
            "-DUSE_LTTNG=OFF",
            "-DUSE_TLS=OFF",
            "-DUSE_RPC_RDMA=OFF",
        ])
        .arg(if gss { "-DUSE_GSS=ON" } else { "-DUSE_GSS=OFF" });
    if let Some(path) = toolchain {
        configure.arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            PathBuf::from(path).display()
        ));
    }
    run(&mut configure);
    run(Command::new("cmake")
        .arg("--build")
        .arg(&build)
        .args(["--target", "ntirpc", "--parallel"])
        .arg(env::var("NUM_JOBS").unwrap_or_else(|_| "2".into())));
    run(Command::new("cmake").arg("--install").arg(&build));
    let include = prefix.join("include/ntirpc");
    let include_parent = prefix.join("include");
    // These definitions match src/CMakeLists.txt in the pinned native source.
    let definitions = ["-D_GNU_SOURCE=1", "-DINET6=1"];
    let mut helper = cc::Build::new();
    helper
        .file("src/auth_helpers.c")
        .include(&include)
        .include(&include_parent)
        .include(&build)
        .pic(true)
        .define("_GNU_SOURCE", "1")
        .define("INET6", "1");
    if gss {
        helper.define("VFSI_RPCSEC_GSS", "1");
        let library = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("krb5-gssapi")
            .expect("rpcsec-gss requires target Kerberos development packages");
        for path in library.include_paths {
            helper.include(path);
        }
    }
    helper.compile("ntirpc_helpers");
    println!(
        "cargo:rustc-link-search=native={}",
        prefix.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=ntirpc");
    pkg_config::Config::new()
        .probe("liburcu-bp")
        .expect("install target liburcu development packages");
    if gss {
        pkg_config::Config::new()
            .probe("krb5-gssapi")
            .expect("install Kerberos development packages");
    }
    println!("cargo:rustc-link-lib=pthread");
    let mut bindings = bindgen::Builder::default()
        .header("src/wrapper.h")
        .clang_arg(format!("--target={target}"))
        .clang_arg(format!("-I{}", include.display()))
        .clang_arg(format!("-I{}", include_parent.display()))
        .clang_arg(format!("-I{}", build.display()))
        .clang_args(definitions)
        .blocklist_type("rpcblist")
        .blocklist_function("xdr_quadruple")
        .blocklist_function("strtold")
        .blocklist_type("_Float32")
        .blocklist_type("_Float32x")
        .blocklist_type("_Float64x")
        .blocklist_type("_Float128")
        .blocklist_type("_Float128x")
        .blocklist_item("strtof(32|64|128)x?(_l)?")
        .blocklist_item("strfromf(32|64|128)x?(_l)?")
        .blocklist_function("qecvt_r")
        .blocklist_function("qfcvt_r")
        .blocklist_function("qecvt")
        .blocklist_function("qfcvt")
        .blocklist_function("qgcvt");
    if let Some(flags) = env::var_os("BINDGEN_EXTRA_CLANG_ARGS") {
        bindings = bindings.clang_args(flags.to_string_lossy().split_whitespace());
    }
    bindings
        .generate()
        .expect("generate bundled ntirpc bindings")
        .write_to_file(out.join("bindings.rs"))
        .expect("write bindings");
    println!("cargo:include={}", include.display());
    println!("cargo:include2={}", include_parent.display());
}
