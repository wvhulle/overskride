use std::{path::PathBuf, process::Command};

const APPLICATION_ID: &str = "page.codeberg.wvhulle.Bindrune";
const CUSTOM_ICON_DIRECTORY: &str = "src/icons/symbolic/actions";
const SCHEMA_SOURCE_DIRECTORY: &str = "../data";
const SCHEMA_FILE: &str = "../data/page.codeberg.wvhulle.Bindrune.gschema.xml";

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
        "cargo:rustc-env=BINDRUNE_COMPILED_SCHEMAS={}",
        schema_directory.display()
    );
}

fn main() {
    println!("cargo:rerun-if-changed={SCHEMA_FILE}");
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some(APPLICATION_ID),
        None::<&str>,
        Some(CUSTOM_ICON_DIRECTORY),
        ["phone", "bluetooth", "folder"],
    );
    compile_settings_schema();
}
