fn main() {
    println!("cargo:rerun-if-changed=assets/cachecat.rc");
    println!("cargo:rerun-if-changed=assets/cachecat.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
        let numeric_version = ["MAJOR", "MINOR", "PATCH"]
            .map(|part| {
                std::env::var(format!("CARGO_PKG_VERSION_{part}")).expect("numeric package version")
            })
            .join(",")
            + ",0";
        let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/cachecat.ico")
            .to_string_lossy()
            .replace('\\', "/");
        let resource = std::fs::read_to_string("assets/cachecat.rc")
            .expect("resource template")
            .replace("assets/cachecat.ico", &icon)
            .replace("@NUMERIC_VERSION@", &numeric_version)
            .replace("@VERSION@", &version);
        let resource_path =
            std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("cachecat.rc");
        std::fs::write(&resource_path, resource).expect("versioned Windows resource");
        embed_resource::compile_for(resource_path, ["cachecat"], embed_resource::NONE)
            .manifest_required()
            .expect("compile CacheCat icon resource");
    }
}
