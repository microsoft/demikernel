// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Extern Linkage
//==============================================================================

extern crate bindgen;

//==============================================================================
// Imports
//==============================================================================

use ::bindgen::{Bindings, Builder};
use ::cc::Build;
use ::std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

//==============================================================================
// Standalone Functions
//==============================================================================

fn main() {
    let out_dir_s: String = env::var("OUT_DIR").unwrap();
    let out_dir: &Path = Path::new(&out_dir_s);

    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    // Get compiler flags.
    let cflags_bytes: Vec<u8> = Command::new("pkg-config")
        .args(&["--cflags", "liburing"])
        .output()
        .unwrap_or_else(|e| panic!("Failed pkg-config cflags: {:?}", e))
        .stdout;
    let cflags: String = String::from_utf8(cflags_bytes).unwrap();

    // Parse include locations.
    let mut header_locations: Vec<&str> = vec![];
    for flag in cflags.split(' ') {
        if flag.starts_with("-I") {
            let header_location = flag[2..].trim();
            header_locations.push(header_location);
        }
    }

    // Get linker flags.
    let ldflags_bytes: Vec<u8> = Command::new("pkg-config")
        .args(&["--libs", "liburing"])
        .output()
        .unwrap_or_else(|e| panic!("Failed pkg-config ldflags: {:?}", e))
        .stdout;
    let ldflags: String = String::from_utf8(ldflags_bytes).unwrap();

    // Parse libraries.
    let mut library_location: Option<&str> = None;
    let mut lib_names: Vec<&str> = vec![];
    for flag in ldflags.split(' ') {
        if flag.starts_with("-L") {
            library_location = Some(&flag[2..]);
        } else if flag.starts_with("-l") {
            lib_names.push(&flag[2..]);
        }
    }

    // Point cargo to libraries.
    println!("cargo:rustc-link-search=native={}", library_location.unwrap());
    for lib_name in &lib_names {
        println!("cargo:rustc-link-lib={}", lib_name);
    }

    // Generate bindings for headers.
    let mut builder: Builder = Builder::default();
    for header_location in &header_locations {
        builder = builder.clang_arg(&format!("-I{}", header_location));
    }
    let bindings: Bindings = builder
        .clang_arg("-mavx")
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .unwrap_or_else(|e| panic!("Failed to generate bindings: {:?}", e));
    let bindings_out: PathBuf = out_dir.join("bindings.rs");
    bindings.write_to_file(bindings_out).expect("Failed to write bindings");

    // Build inlined functions.
    let mut builder: Build = cc::Build::new();
    builder.opt_level(3);
    builder.pic(true);
    builder.flag("-march=native");
    builder.file("inlined.c");
    for header_location in &header_locations {
        builder.include(header_location);
    }
    builder.compile("inlined");
}
