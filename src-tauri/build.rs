fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/needle");
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=needle");
    println!("cargo:rustc-link-lib=c++");
    tauri_build::build()
}
