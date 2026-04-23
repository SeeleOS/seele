fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-no-pie");
    println!(
        "cargo:rustc-link-arg=-T{manifest_dir}/linker-x86_64.ld"
    );
    println!("cargo:rerun-if-changed=linker-x86_64.ld");
}
