fn main() {
    println!("cargo:rustc-link-search=.");
    println!("cargo:rerun-if-changed=linker-arm64.ld");
}
