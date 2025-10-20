fn main() {
    // Only add the defmt linker script when the defmt feature is enabled
    #[cfg(feature = "defmt")]
    println!("cargo:rustc-link-arg=-Tdefmt.x");
}
