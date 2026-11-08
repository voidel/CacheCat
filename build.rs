fn main() {
    println!("cargo:rerun-if-changed=assets/cachecat.rc");
    println!("cargo:rerun-if-changed=assets/cachecat.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("assets/cachecat.rc", ["cachecat"], embed_resource::NONE)
            .manifest_required()
            .expect("compile CacheCat icon resource");
    }
}
