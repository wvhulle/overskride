use std::path::{Path, PathBuf};
use std::process::Command;

const BLUEPRINT_DIRECTORY: &str = "src/gtk";
const RESOURCE_SOURCE_DIRECTORY: &str = "src";
const RESOURCE_MANIFEST: &str = "src/overskride.gresource.xml";
const RESOURCE_BUNDLE_NAME: &str = "overskride.gresource";

fn blueprint_paths(blueprint_directory: &Path) -> Vec<PathBuf> {
    let mut blueprint_paths: Vec<_> = std::fs::read_dir(blueprint_directory)
        .expect("cannot read the blueprint directory")
        .filter_map(Result::ok)
        .map(|directory_entry| directory_entry.path())
        .filter(|candidate_path| candidate_path.extension().is_some_and(|extension| extension == "blp"))
        .collect();
    blueprint_paths.sort();
    blueprint_paths
}

fn compile_blueprints(output_directory: &Path) {
    let compiler_status = Command::new("blueprint-compiler")
        .arg("batch-compile")
        .arg(output_directory)
        .arg(RESOURCE_SOURCE_DIRECTORY)
        .args(blueprint_paths(Path::new(BLUEPRINT_DIRECTORY)))
        .status()
        .expect("cannot run blueprint-compiler, is it on PATH?");
    assert!(compiler_status.success(), "blueprint-compiler failed");
}

fn main() {
    let output_directory = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));

    println!("cargo:rerun-if-changed={BLUEPRINT_DIRECTORY}");
    println!("cargo:rerun-if-changed={RESOURCE_SOURCE_DIRECTORY}/icons");
    println!("cargo:rerun-if-changed={RESOURCE_MANIFEST}");

    compile_blueprints(&output_directory);

    glib_build_tools::compile_resources(
        &[output_directory.as_path(), Path::new(RESOURCE_SOURCE_DIRECTORY)],
        RESOURCE_MANIFEST,
        RESOURCE_BUNDLE_NAME,
    );
}
