use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::process::{Command, Stdio};

fn prepare_staticlib_toml(out_dir: &str) -> std::io::Result<String> {
    let manifest = format!("{}/staticlib/Cargo.toml", out_dir);
    fs::create_dir_all(format!("{}/staticlib", out_dir))
        .expect("Could not create directory for staticlib");
    let toml = fs::read_to_string("staticlib/Cargo.nottoml")?;

    // Adapt path
    let mut lib = env::current_dir()?;
    lib.push("src");
    lib.push("lib.rs");
    let toml = toml.replace(
        "../src/lib.rs",
        lib.to_str().expect("Invalid staticlib path"),
    );

    let mut toml_out = File::create(&manifest)?;
    toml_out.write_all(toml.as_bytes())?;
    //fs::copy("", format!("{}/staticlib/Cargo.toml", out_dir)).expect("Could not copy staticlib Cargo.toml");
    Ok(manifest)
}

fn build_backend() {
    println!("Building Backend!");
    // Get envvars from cargo
    let out_dir = env::var("OUT_DIR").unwrap();
    let full_target_dir = format!("{}/target_static", out_dir);
    let profile = env::var("PROFILE").expect("PROFILE was not set");

    // Set the target. Can be overwritten via env-var.
    // If feature autokernel is enabled, automatically 'convert' hermit to hermit-kernel target.
    let target = {
        println!("cargo:rerun-if-env-changed=RFTRACE_TARGET_TRIPLE");
        env::var("RFTRACE_TARGET_TRIPLE").unwrap_or_else(|_| {
            let default = env::var("TARGET").unwrap();
            #[cfg(not(feature = "autokernel"))]
            return default;
            #[cfg(feature = "autokernel")]
            if default == "x86_64-unknown-hermit" {
                "x86_64-unknown-none-hermitkernel".to_owned()
            } else {
                default
            }
        })
    };
    println!("Compiling for target {}", target);

    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    // Compile for the same target as the parent-lib
    cmd.args(&["--target", &target]);

    // Output all build artifacts in output dir of parent-lib
    cmd.args(&["--target-dir", &full_target_dir]);

    // Use custom manifest, which defines that this compilation is a staticlib
    // crates.io allows only one Cargo.toml per package, so copy here
    let manifest =
        prepare_staticlib_toml(&out_dir).expect("Could not prepare staticlib toml file!");
    cmd.args(&["--manifest-path", &manifest]);

    // Enable the staticlib feature, so we can do #[cfg(feature='staticlib')] gate our code
    // Pass-through interruptsafe and reexportsymbols features
    let mut features = "staticlib".to_owned();
    #[cfg(feature = "interruptsafe")]
    features.push_str(",interruptsafe");
    cmd.args(&["--features", &*features]);

    // Always output color, so eventhough we are cargo-in-cargo, we get nice error messages on build fail
    cmd.args(&["--color", "always"]);

    // Redirect stdout and err, so we have live progress of compilation (?)
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // Build core, needed when compiling against a kernel-target, such as x86_64-unknown-none-hermitkernel.
    // parent's cargo does NOT expose -Z flags as envvar, we therefore use a feature flag for this
    #[cfg(feature = "buildcore")]
    cmd.args(&["-Z", "build-std=core"]); // should be build std,alloc?

    // Compile staticlib as release if included in release build.
    if profile == "release" {
        cmd.arg("--release");
    }

    // Ensure rustflags does NOT contain instrument-mcount!
    let rustflags = env::var("RUSTFLAGS").unwrap_or_default();
    if rustflags.contains("mcount") {
        println!("WARNING: RUSTFLAGS contains mcount, trying to remove the key.");
        cmd.env(
            "RUSTFLAGS",
            rustflags
                .replace("-Z instrument-mcount", "")
                .replace("-Z instrument_mcount", ""),
        );
    }

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
