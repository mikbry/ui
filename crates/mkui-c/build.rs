use std::env;
use std::path::PathBuf;

fn main() {
    let package_name = env::var("CARGO_PKG_NAME").unwrap();
    let output_file = target_dir()
        .join("include")
        .join(format!("{}.h", package_name.replace('-', "_")));

    std::fs::create_dir_all(output_file.parent().unwrap()).unwrap();

    // Since we're providing a manual header, just create a simple one
    let manual_header = r#"/* This header is manually maintained - see include/mkui_c.h for the actual API */
#ifndef MKUI_C_H_GENERATED
#define MKUI_C_H_GENERATED

#include "mkui_c.h"

#endif /* MKUI_C_H_GENERATED */
"#;

    std::fs::write(&output_file, manual_header).expect("Unable to write header file");

    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=build.rs");
}

fn target_dir() -> PathBuf {
    if let Ok(target) = env::var("CARGO_TARGET_DIR") {
        PathBuf::from(target)
    } else {
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("target")
    }
}
