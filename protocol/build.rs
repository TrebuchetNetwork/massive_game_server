use flatc_rust::{Args, Flatc};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable not set.");
    let schema_file = Path::new(&manifest_dir).join("schemas/game.fbs");
    if !schema_file.exists() {
        panic!(
            "FlatBuffers schema file not found at '{}'.",
            schema_file.display()
        );
    }
    println!("cargo:rerun-if-changed={}", schema_file.display());

    let out_dir_str = std::env::var("OUT_DIR").expect("OUT_DIR environment variable not set.");
    let output_dir = Path::new(&out_dir_str).join("flatbuffers_generated");
    std::fs::create_dir_all(&output_dir).unwrap_or_else(|err| {
        panic!(
            "Failed to create FlatBuffers output directory '{}': {}",
            output_dir.display(),
            err
        )
    });

    let flatc_compiler = Flatc::from_env_path();
    let args = Args {
        lang: "rust",
        inputs: &[schema_file.as_path()],
        out_dir: output_dir.as_path(),
        ..Default::default()
    };
    if let Err(err) = flatc_compiler.run(args) {
        panic!(
            "flatc-rust execution failed for '{}' -> '{}': {:?}",
            schema_file.display(),
            output_dir.display(),
            err
        );
    }

    let expected_generated_file = output_dir.join("game_generated.rs");
    if !expected_generated_file.exists() {
        panic!(
            "Expected FlatBuffers generated file not found at '{}'.",
            expected_generated_file.display()
        );
    }
}
