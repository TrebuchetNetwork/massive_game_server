// server/build.rs
use std::path::Path;
use flatc_rust::{Flatc, Args};

fn main() {
    // Generate build-time information (existing)
    built::write_built_file().expect("Failed to acquire build-time information");
    println!("cargo:rerun-if-changed=build.rs"); // Important for build script itself

    // --- FlatBuffers Compilation (adapted from webrtc_shooter_server) ---
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable not set.");

    // Assuming game.fbs is at massive_game_server/server/schemas/game.fbs
    // User confirmed this path in the latest interaction.
    let schema_file = Path::new(&manifest_dir).join("schemas/game.fbs");

    if !schema_file.exists() {
        panic!(
            "FlatBuffers schema file not found at {:?}. Ensure 'schemas/game.fbs' exists in the server directory.",
            schema_file
        );
    }

    // Output to OUT_DIR for correctness with Cargo
    let out_dir_str = std::env::var("OUT_DIR").unwrap();
    let output_dir = Path::new(&out_dir_str).join("flatbuffers_generated");

    std::fs::create_dir_all(&output_dir)
        .unwrap_or_else(|e| panic!("Failed to create FlatBuffers output directory {:?}: {}", output_dir, e));

    println!("cargo:rerun-if-changed={}", schema_file.to_str().unwrap());

    println!(
        "Attempting to compile FlatBuffers schema: {} into output directory: {}",
        schema_file.display(),
        output_dir.display()
    );

    let flatc_compiler = Flatc::from_env_path();
    let schema_file_ref: &Path = schema_file.as_path();
    let output_dir_ref: &Path = output_dir.as_path();

    let args = Args {
        lang: "rust",
        inputs: &[schema_file_ref],
        out_dir: output_dir_ref,
        ..Default::default()
    };

    // Corrected typo: flatc_compiler instead of flatccompiler
    match flatc_compiler.run(args) {
        Ok(_) => {
            println!("cargo:info=FlatBuffers schema compilation successful (invoked via flatc-rust).");
        }
        Err(e) => {
            panic!(
                "flatc-rust execution failed: {:?}. Schema: '{}', Output: '{}'",
                e,
                schema_file.display(),
                output_dir.display()
            );
        }
    }

    let expected_generated_file = output_dir.join("game_generated.rs");
    if !expected_generated_file.exists() {
        panic!(
            "Expected FlatBuffers generated file not found at {:?}",
            expected_generated_file
        );
    } else {
        println!("cargo:info=Successfully verified that the generated FlatBuffers file exists at {:?}", expected_generated_file);
    }
}
