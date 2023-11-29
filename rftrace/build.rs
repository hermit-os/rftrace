use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
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

    // Be very verbose
    //cmd.arg("-vv");

    // Redirect stdout and err, so we have live progress of compilation (?)
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    cmd.args(&[
        "-Zbuild-std=core",
        "-Zbuild-std-features=compiler-builtins-mem",
    ]);

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

    let dist_dir = format!("{}/{}/{}", &full_target_dir, &target, &profile);

    retain_symbols(
        Path::new(&format!("{}/librftrace_backend.a", &dist_dir)),
        &[
            "mcount",
            "rftrace_backend_disable",
            "rftrace_backend_enable",
            "rftrace_backend_get_events",
            "rftrace_backend_get_events_index",
            "rftrace_backend_init",
        ],
    );

    // Link parent-lib against this staticlib
    println!("cargo:rustc-link-search=native={}", &dist_dir);
    println!("cargo:rustc-link-lib=static=rftrace_backend");

    println!("cargo:rerun-if-changed=staticlib/Cargo.toml");
    println!("cargo:rerun-if-changed=src/backend.rs");
    println!("cargo:rerun-if-changed=src/interface.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}

fn main() {
    build_backend();
}

/// Makes all internal symbols private to avoid duplicated symbols.
///
/// This allows us to have rftrace's copy of `core` alongside other potential copies in the final binary.
/// This is important when combining different versions of `core`.
/// Newer versions of `rustc` will throw an error on duplicated symbols.
// Adapted from Hermit.
pub fn retain_symbols(archive: &Path, symbols: &[&str]) {
    use std::fmt::Write;

    let prefix = "rftrace";

    let symbol_renames = symbols.iter().fold(String::new(), |mut output, symbol| {
        let _ = writeln!(output, "{prefix}_{symbol} {symbol}");
        output
    });

    let rename_path = archive.with_extension("redefine_syms");
    fs::write(&rename_path, symbol_renames).unwrap();

    let objcopy = binutil("objcopy").unwrap();
    let status = Command::new(&objcopy)
        .arg("--prefix-symbols")
        .arg(format!("{prefix}_"))
        .arg(archive)
        .status()
        .unwrap();
    assert!(status.success());
    let status = Command::new(&objcopy)
        .arg("--redefine-syms")
        .arg(&rename_path)
        .arg(archive)
        .status()
        .unwrap();
    assert!(status.success());

    fs::remove_file(&rename_path).unwrap();
}

/// Returns the path to the requested binutil from the llvm-tools component.
// Adapted from Hermit.
fn binutil(name: &str) -> Result<PathBuf, String> {
    let exe_suffix = env::consts::EXE_SUFFIX;
    let exe = format!("llvm-{name}{exe_suffix}");

    let path = llvm_tools::LlvmTools::new()
		.map_err(|err| match err {
			llvm_tools::Error::NotFound =>
				"Could not find llvm-tools component\n\
				\n\
				Maybe the rustup component `llvm-tools` is missing? Install it through: `rustup component add llvm-tools`".to_string()
			,
			err => format!("{err:?}"),
		})?
		.tool(&exe)
		.ok_or_else(|| format!("could not find {exe}"))?;

    Ok(path)
}
