fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // The main thread runs the game: give it Linux's 8 MiB on Windows, whose
    // default is 1 MiB.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/STACK:8388608");
    }
}
