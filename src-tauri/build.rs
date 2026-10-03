fn main() {
    tauri_build::build();
    // Tauri links its Windows manifest/resources only into binary targets.
    // The native window test also needs Common Controls v6, so make that same
    // generated resource available to the cfg(test) native link in lib.rs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!(
            "cargo:rustc-link-search=native={}",
            std::env::var("OUT_DIR").unwrap()
        );
    }
}
