// server/build.rs
use flatc_rust::{Args, Flatc};
use std::fs;
use std::path::Path;

fn main() {
    // Generate build-time information (existing)
    built::write_built_file().expect("Failed to acquire build-time information");
    println!("cargo:rerun-if-changed=build.rs"); // Important for build script itself

    // --- FlatBuffers Compilation (adapted from webrtc_shooter_server) ---
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable not set.");

    // Canonical schema source lives in protocol/schemas in this workspace.
    // For published crate tarballs, only server/schemas/game.fbs is available.
    let canonical_schema_file = Path::new(&manifest_dir).join("../protocol/schemas/game.fbs");
    let server_schema_mirror = Path::new(&manifest_dir).join("schemas/game.fbs");

    if !server_schema_mirror.exists() {
        panic!(
            "Server schema mirror file not found at {:?}. Ensure 'server/schemas/game.fbs' exists.",
            server_schema_mirror
        );
    }

    let schema_file = if canonical_schema_file.exists() {
        let canonical_schema_bytes = fs::read(&canonical_schema_file).unwrap_or_else(|e| {
            panic!(
                "Failed to read canonical schema file {:?}: {}",
                canonical_schema_file, e
            )
        });
        let mirror_schema_bytes = fs::read(&server_schema_mirror).unwrap_or_else(|e| {
            panic!(
                "Failed to read server schema mirror file {:?}: {}",
                server_schema_mirror, e
            )
        });
        if canonical_schema_bytes != mirror_schema_bytes {
            panic!(
                "Schema drift detected between {:?} and {:?}. Sync server mirror from canonical protocol schema before building.",
                canonical_schema_file, server_schema_mirror
            );
        }
        canonical_schema_file
    } else {
        println!(
            "cargo:warning=Canonical schema {:?} not found; using in-crate mirror {:?}.",
            canonical_schema_file, server_schema_mirror
        );
        server_schema_mirror.clone()
    };

    // Output to OUT_DIR for correctness with Cargo
    let out_dir_str = std::env::var("OUT_DIR").unwrap();
    let output_dir = Path::new(&out_dir_str).join("flatbuffers_generated");

    std::fs::create_dir_all(&output_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to create FlatBuffers output directory {:?}: {}",
            output_dir, e
        )
    });

    println!("cargo:rerun-if-changed={}", schema_file.to_str().unwrap());
    println!(
        "cargo:rerun-if-changed={}",
        server_schema_mirror.to_str().unwrap()
    );

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
            println!(
                "cargo:info=FlatBuffers schema compilation successful (invoked via flatc-rust)."
            );
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
        println!(
            "cargo:info=Successfully verified that the generated FlatBuffers file exists at {:?}",
            expected_generated_file
        );
    }
}
