fn main() {
    println!("cargo:rerun-if-changed=assets/airbridge.res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let path = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/airbridge.res");
        println!("cargo:rustc-link-arg-bin=airbridge-ui={}", path.display());
    }
}
