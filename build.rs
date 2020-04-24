use std::env;
use std::process::{Command, Stdio};


fn main() {
    println!("Exec build.rs!");
    // Get envvars from cargo
    let out_dir = env::var("OUT_DIR").unwrap();
    let target = env::var("TARGET").unwrap();
    //let target = "x86_64-unknown-hermit-kernel";
    let full_target_dir = format!("{}/target_static", out_dir);
    let profile = env::var("PROFILE").expect("PROFILE was not set");
    let opt_level = env::var("OPT_LEVEL").expect("OPT_LEVEL was not set");

    let mut cmd = Command::new("cargo");
    // we need nightly, since we use named-profiles. Crash when we dont have it as default.
    //cmd.arg("+nightly");
    cmd.arg("build");

    // Compile for the same target as the parent-lib
    cmd.args(&["--target", &target]);

    // Output all build artifacts in output dir of parent-lib
    cmd.args(&["--target-dir", &full_target_dir]);

    // Use custom manifest, which defines that this compilation is a staticlib
    cmd.args(&["--manifest-path", "staticlib/Cargo.toml"]);

    // Enable the staticlib feature, so we can do #[cfg(feature='staticlib')] gate our code
    cmd.args(&["--features", "staticlib"]);

    // Always output color, so eventhough we are cargo-in-cargo, we get nice error messages on build fail
    cmd.args(&["--color", "always"]);

    // Be very verbose
    //cmd.arg("-vv");

    // Redirect stdout and err, so we have live progress of compilation (?)
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // Build standard, needed for hermitcore.. TODO: avoid rebuilding and use parent-libs one (?)
    // parent's cargo does NOT expose -Z flags as envvar, but for RustyHermit, we want to be able to pass along the 'build-std' flags.
    // We therefore use a feature flag for this
    #[cfg(feature="buildstd")]
    cmd.args(&["-Z", "build-std=std,core,alloc,panic_abort"]);

    // we have to MATCH THE HERMITKERNEL optlevel! else we get duplicate symbols! 
    // (runtime_entry, __rust_drop_panic, rust_begin_unwind, rust_panic, ...)
    // TODO: WHY?
    cmd.args(&["-Z", "unstable-options"]);
    cmd.args(&["--profile", &format!("{}-opt{}", &profile, &opt_level)]);
    

    // Execute and get status.
    println!("Starting sub-cargo");
    let status = cmd.status().expect("Unable to build tracer's static lib!");
    
    // Panic on fail, so the build aborts and the error messages are printed
    assert!(status.success(), "Unable to build tracer's static lib!");
    println!("Sub-cargo successful!");

    // Link parent-lib against this staticlib
    println!("cargo:rustc-link-search=native={}/{}/{}-opt{}/", &full_target_dir, &target, &profile, &opt_level);
    println!("cargo:rustc-link-lib=static=tracer_rs_static");

    // Just for dev
    println!("cargo:rerun-if-changed=staticlib/Cargo.toml");
    println!("cargo:rerun-if-changed=src/trace.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
