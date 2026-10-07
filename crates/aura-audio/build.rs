fn main() {
    #[cfg(target_os = "macos")]
    {
        // Builds native/macos/AuraCapture and links it with the Swift runtime.
        swift_rs::SwiftLinker::new("15.0")
            .with_package("AuraCapture", "../../native/macos/AuraCapture")
            .link();
        println!("cargo:rerun-if-changed=../../native/macos/AuraCapture/Sources");
        println!("cargo:rerun-if-changed=../../native/macos/AuraCapture/Package.swift");
    }
}
