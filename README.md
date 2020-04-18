# Build Process Internals
- cargo build
    - runs build.rs::build()
        - compiles part of the code as a native staticlib, by using the Cargo.toml in `/staticlib` and passing the staticlib feature to cargo build
    - compiles the library as normal, without the staticlib feature and links the 'precompiled' static library from the previous step.

You can compile only static lib manually with
`cargo build --manifest-path staticlib/Cargo.toml --target-dir target_static --features staticlib -vv`

# TODO:
- stack targetdirs
