use std::path::PathBuf;
use std::process::Command;

const RESOURCE_SOURCE_DIRECTORY: &str = "src";
const RESOURCE_MANIFEST: &str = "src/overskride.gresource.xml";
const RESOURCE_BUNDLE_NAME: &str = "overskride.gresource";
const SCHEMA_SOURCE_DIRECTORY: &str = "../data";
const SCHEMA_FILE: &str = "../data/io.github.kaii_lb.Overskride.gschema.xml";

fn compile_settings_schema() {
    let schema_directory =
        PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("schemas");
    std::fs::create_dir_all(&schema_directory).expect("cannot create the schema directory");
    let compiler_status = Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg("--targetdir")
        .arg(&schema_directory)
        .arg(SCHEMA_SOURCE_DIRECTORY)
        .status()
        .expect("cannot run glib-compile-schemas, is glib installed?");
    assert!(compiler_status.success(), "glib-compile-schemas failed");
    println!(
        "cargo:rustc-env=OVERSKRIDE_COMPILED_SCHEMAS={}",
        schema_directory.display()
    );
}

fn main() {
    println!("cargo:rerun-if-changed={RESOURCE_SOURCE_DIRECTORY}/icons");
    println!("cargo:rerun-if-changed={RESOURCE_MANIFEST}");
    println!("cargo:rerun-if-changed={SCHEMA_FILE}");
    glib_build_tools::compile_resources(
        &[RESOURCE_SOURCE_DIRECTORY],
        RESOURCE_MANIFEST,
        RESOURCE_BUNDLE_NAME,
    );
    compile_settings_schema();
}
