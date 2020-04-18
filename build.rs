use std::env;
use std::process::{Command, Stdio};

fn main() {
    println!("Exec build.rs!");
    #[cfg(feature = "buildinternal")]
    println!("Build internal set!!");
    #[cfg(not(feature = "buildinternal"))]
    println!("Build internal NOT set!!");

    #[cfg(not(feature = "buildinternal"))]
    {
        let _out_dir = env::var("OUT_DIR").unwrap();
        let target_dir = "target_static";
        //let full_target_dir = format!("{}/target_static", out_dir);
        //let profile = env::var("PROFILE").expect("PROFILE was not set");

        println!("Stargins sub-cargo");
        let status = Command::new("cargo")
            //.current_dir("../libhermit-rs")
            .arg("build")
            .arg("--color")
            .arg("always")
            .arg("--target-dir")
            .arg(target_dir)
            .arg("--manifest-path")
            .arg("staticlib/Cargo.toml")
            .arg("--features")
            .arg("staticlib")
            .arg("-vv")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .expect("Unable to build libtracerstatic");
        assert!(status.success());

        println!("Finished sub-cargo");
        println!("{:?}", status);

        println!("cargo:rustc-link-search=native={}/debug/", target_dir);
        println!("cargo:rustc-link-lib=static=tracer_rs_static");
    }
}
