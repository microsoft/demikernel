// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Extern Linkage
//==============================================================================

extern crate bindgen;
extern crate cc;

//==============================================================================
// Imports
//==============================================================================

use anyhow::Result;
use bindgen::{
    Bindings,
    Builder,
};
use cc::Build;
use std::{
    env,
    path::{
        Path,
        PathBuf,
    },
};

//==============================================================================
// Standalone Functions
//==============================================================================

static WRAPPER_HEADER_NAME: &str = "wrapper.h";
static INLINED_C_NAME: &str = "inlined.c";
static OUT_DIR_VAR: &str = "OUT_DIR";
static XDP_PATH_VAR: &str = "XDP_PATH";
static INCLUDE_DIR: &str = "\\include";
static LIB_DIR: &str = "\\lib";
static XDP_API_LIB: &str = "xdpapi";
static SAL_BLOCKLIST_REGEX: &str = r".*SAL.*";
static TYPE_BLOCKLIST: [&str; 8] = [
    ".*OVERLAPPED.*",
    "HANDLE",
    "HRESULT",
    "IN_ADDR",
    "IN6_ADDR",
    "in_addr.*",
    "in6_addr.*",
    "_?XDP_INET_ADDR",
];
static FILE_ALLOWLIST_REGEX: &str = r".*xdp.*";

fn main() -> Result<()> {
    let out_dir_s: String = env::var(OUT_DIR_VAR).unwrap();
    let out_dir: &Path = Path::new(&out_dir_s);

    let libxdp_path: String = env::var(XDP_PATH_VAR)?;

    let include_path: String = format!("{}{}", &libxdp_path, INCLUDE_DIR);
    let lib_path: String = format!("{}{}", &libxdp_path, LIB_DIR);

    println!("include_path: {}", include_path);
    println!("lib_path: {}", lib_path);

    // Point cargo to the libraries.
    println!("cargo:rustc-link-search={}", lib_path);
    println!("cargo:rustc-link-lib=dylib={}", XDP_API_LIB);

    let mut builder = Builder::default();
    for t in TYPE_BLOCKLIST.iter() {
        builder = builder.blocklist_type(t);
    }

    // Generate bindings for headers.
    let bindings: Bindings = builder
        .clang_arg(&format!("-I{}", include_path))
        .clang_arg("-mavx")
        .header(WRAPPER_HEADER_NAME)
        // NB SAL defines still get included despite having no functional impact.
        .blocklist_item(SAL_BLOCKLIST_REGEX)
        .allowlist_file(FILE_ALLOWLIST_REGEX)
        // Allow the inline function wrappers to be generated.
        .allowlist_file(format!(".*{}", WRAPPER_HEADER_NAME))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .unwrap_or_else(|e| panic!("Failed to generate bindings: {:?}", e));
    let bindings_out: PathBuf = out_dir.join("bindings.rs");
    bindings.write_to_file(bindings_out).expect("Failed to write bindings");

    // Build inlined functions.
    let mut builder: Build = cc::Build::new();
    builder.opt_level(3);
    builder.pic(true);
    builder.file(INLINED_C_NAME);
    builder.include(include_path);
    builder.compile("inlined");

    Ok(())
}
