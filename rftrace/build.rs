use std::env;
use std::process::{Command, Stdio};

fn build_backend() {
    println!("Building Backend!");
    // Get envvars from cargo
    let out_dir = env::var("OUT_DIR").unwrap();
    let full_target_dir = format!("{}/target_static", out_dir);
    let profile = env::var("PROFILE").expect("PROFILE was not set");

    // Set the target. file if shortcut-feature hermit is chosen.
    // Else, allow overriding target via env-var
    #[cfg(feature = "hermit")]
    let target = "x86_64-unknown-hermit-kernel";
    #[cfg(not(feature = "hermit"))]
    let target = {
        println!("cargo:rerun-if-env-changed=RFTRACE_TARGET_TRIPLE");
        env::var("RFTRACE_TARGET_TRIPLE").unwrap_or_else(|_| env::var("TARGET").unwrap())
    };

    let mut cmd = Command::new("cargo");
    // We use nightly features, so always enable it
    cmd.arg("+nightly");
    cmd.arg("build");

    // Compile for the same target as the parent-lib
    cmd.args(&["--target", &target]);

    // Output all build artifacts in output dir of parent-lib
    cmd.args(&["--target-dir", &full_target_dir]);

    // Use custom manifest, which defines that this compilation is a staticlib
    cmd.args(&["--manifest-path", "staticlib/Cargo.toml"]);

    // Enable the staticlib feature, so we can do #[cfg(feature='staticlib')] gate our code
    // Pass-through interruptsafe feature
    #[cfg(feature = "interruptsafe")]
    cmd.args(&["--features", "staticlib,interruptsafe"]);
    #[cfg(not(feature = "interruptsafe"))]
    cmd.args(&["--features", "staticlib"]);

    // Always output color, so eventhough we are cargo-in-cargo, we get nice error messages on build fail
    cmd.args(&["--color", "always"]);

    // Be very verbose
    //cmd.arg("-vv");

    // Redirect stdout and err, so we have live progress of compilation (?)
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // Build core, needed when compiling against a kernel-target, such as x86_64-unknown-hermit-kernel.
    // parent's cargo does NOT expose -Z flags as envvar, we therefore use a feature flag for this
    #[cfg(feature = "buildcore")]
    cmd.args(&["-Z", "build-std=core"]); // should be build std,alloc?

    // Compile staticlib as release if included in release build.
    if profile == "release" {
        cmd.arg("--release");
    }

    // Ensure rustflags does NOT contain instrument-mcount!
    cmd.env(
        "RUSTFLAGS",
        env::var("RUSTFLAGS")
            .unwrap_or("".into())
            .replace("-Z instrument-mcount", ""),
    );

    // Execute and get status.
    println!("Starting sub-cargo");
    let status = cmd.status().expect("Unable to build tracer's static lib!");

    // Panic on fail, so the build aborts and the error messages are printed
    assert!(status.success(), "Unable to build tracer's static lib!");
    println!("Sub-cargo successful!");

    // Link parent-lib against this staticlib
    println!(
        "cargo:rustc-link-search=native={}/{}/{}/",
        &full_target_dir, &target, &profile
    );
    println!("cargo:rustc-link-lib=static=rftrace_backend");

    println!("cargo:rerun-if-changed=staticlib/Cargo.toml");
    println!("cargo:rerun-if-changed=src/backend.rs");
    println!("cargo:rerun-if-changed=src/interface.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}

fn main() {
    build_backend();
}
