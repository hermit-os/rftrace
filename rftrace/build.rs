use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs};

fn build_backend() {
    println!("Building Backend!");
    // Get envvars from cargo
    let out_dir = env::var("OUT_DIR").unwrap();
    let full_target_dir = format!("{}/target_static", out_dir);

    let target = "x86_64-unknown-none";

    let mut cmd = cargo();
    cmd.arg("+nightly");
    cmd.arg("rustc");

    cmd.args(&["--target", target]);

    // Output all build artifacts in output dir of parent-lib
    cmd.args(&["--target-dir", &full_target_dir]);

    // Enable the staticlib feature, so we can do #[cfg(feature='staticlib')] gate our code
    // Pass-through interruptsafe and reexportsymbols features
    cmd.arg("--features=staticlib");
    if env::var_os("CARGO_FEATURE_INTERRUPTSAFE").is_some() {
        cmd.arg("--features=interruptsafe");
    }

    // Always output color, so eventhough we are cargo-in-cargo, we get nice error messages on build fail
    cmd.args(&["--color", "always"]);

    // Redirect stdout and err, so we have live progress of compilation (?)
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    cmd.args(&[
        "-Zbuild-std=core",
        "-Zbuild-std-features=compiler-builtins-mem",
    ]);

    cmd.arg("--release");

    cmd.arg("--");

    cmd.arg("-Cpanic=abort");

    cmd.env_remove("RUSTFLAGS");
    cmd.env_remove("CARGO_ENCODED_RUSTFLAGS");

    // Execute and get status.
    println!("Starting sub-cargo");
    let status = cmd.status().expect("Unable to build tracer's static lib!");

    // Panic on fail, so the build aborts and the error messages are printed
    assert!(status.success(), "Unable to build tracer's static lib!");
    println!("Sub-cargo successful!");

    let dist_dir = format!("{}/{}/release", &full_target_dir, &target);

    retain_symbols(
        Path::new(&format!("{}/librftrace.a", &dist_dir)),
        HashSet::from([
            "mcount",
            "rftrace_backend_disable",
            "rftrace_backend_enable",
            "rftrace_backend_get_events",
            "rftrace_backend_get_events_index",
            "rftrace_backend_init",
        ]),
    );

    // Link parent-lib against this staticlib
    println!("cargo:rustc-link-search=native={}", &dist_dir);
    println!("cargo:rustc-link-lib=static=rftrace");

    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src/backend.rs");
    println!("cargo:rerun-if-changed=src/interface.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}

fn main() {
    if env::var_os("CARGO_FEATURE_STATICLIB").is_some() {
        return;
    }

    build_backend();
}

/// Returns the Rustup proxy for Cargo.
// Adapted from Hermit.
fn cargo() -> Command {
    let cargo = {
        let exe = format!("cargo{}", env::consts::EXE_SUFFIX);
        // On windows, the userspace toolchain ends up in front of the rustup proxy in $PATH.
        // To reach the rustup proxy nonetheless, we explicitly query $CARGO_HOME.
        let mut cargo_home = PathBuf::from(env::var_os("CARGO_HOME").unwrap());
        cargo_home.push("bin");
        cargo_home.push(&exe);
        if cargo_home.exists() {
            cargo_home
        } else {
            PathBuf::from(exe)
        }
    };

    let mut cargo = Command::new(cargo);

    // Remove rust-toolchain-specific environment variables from kernel cargo
    cargo.env_remove("LD_LIBRARY_PATH");
    env::vars()
        .filter(|(key, _value)| key.starts_with("CARGO") || key.starts_with("RUST"))
        .for_each(|(key, _value)| {
            cargo.env_remove(&key);
        });

    cargo
}

/// Makes all internal symbols private to avoid duplicated symbols.
///
/// This allows us to have rftrace's copy of `core` alongside other potential copies in the final binary.
/// This is important when combining different versions of `core`.
/// Newer versions of `rustc` will throw an error on duplicated symbols.
// Adapted from Hermit.
pub fn retain_symbols(archive: &Path, mut exported_symbols: HashSet<&str>) {
    use std::fmt::Write;

    let prefix = "rftrace";

    let all_symbols = {
        let objcopy = binutil("nm").unwrap();
        let output = Command::new(&objcopy)
            .arg("--export-symbols")
            .arg(archive)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };

    let symbol_renames = all_symbols
        .lines()
        .fold(String::new(), |mut output, symbol| {
            if exported_symbols.remove(symbol) {
                return output;
            }

            if let Some(symbol) = symbol.strip_prefix("_ZN") {
                let prefix_len = prefix.len();
                let _ = writeln!(output, "_ZN{symbol} _ZN{prefix_len}{prefix}{symbol}",);
            } else {
                let _ = writeln!(output, "{symbol} {prefix}_{symbol}");
            }
            output
        });
    assert!(exported_symbols.is_empty());

    let rename_path = archive.with_extension("redefine_syms");
    fs::write(&rename_path, symbol_renames).unwrap();

    let objcopy = binutil("objcopy").unwrap();
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
				format!("Could not find llvm-tools component\n\
				\n\
				Maybe the rustup component `llvm-tools` is missing? Install it through: `rustup component add --toolchain={} llvm-tools`", env::var("RUSTUP_TOOLCHAIN").unwrap()).to_string()
			,
			err => format!("{err:?}"),
		})?
		.tool(&exe)
		.ok_or_else(|| format!("could not find {exe}"))?;

    Ok(path)
}
