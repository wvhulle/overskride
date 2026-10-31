const RESOURCE_SOURCE_DIRECTORY: &str = "src";
const RESOURCE_MANIFEST: &str = "src/overskride.gresource.xml";
const RESOURCE_BUNDLE_NAME: &str = "overskride.gresource";

fn main() {
    println!("cargo:rerun-if-changed={RESOURCE_SOURCE_DIRECTORY}/icons");
    println!("cargo:rerun-if-changed={RESOURCE_MANIFEST}");
    glib_build_tools::compile_resources(
        &[RESOURCE_SOURCE_DIRECTORY],
        RESOURCE_MANIFEST,
        RESOURCE_BUNDLE_NAME,
    );
}
