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

use ::bindgen::{Bindings, Builder};
use ::cc::Build;
use ::std::{
    env,
    io::{Result, Error, ErrorKind},
    path::{Path, PathBuf},
};

//==============================================================================
// Standalone Functions
//==============================================================================

static WRAPPER_HEADER_NAME: &str = "wrapper.h";
static INLINED_C_NAME: &str = "inlined.c";
static OUT_DIR_VAR: &str = "OUT_DIR";
static XDP_PATH_VAR: &str = "XDP_PATH";
static PROGRAM_FILES_VAR: &str = "ProgramFiles";
static LIB_DIR: &str = "lib";
static INC_DIR: &str = "include";
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


/// Returns a tuple containing (lib_path, inc_path) based on the XDP installation path `p`, or an
/// error if such resolution fails.
fn resolve_xdp_path(p: &Path) -> Result<(PathBuf, PathBuf)> {
    // Validate that a path is a reachable directory, or error out.
    let exists = |p: &Path| -> Result<()> {
        if !(p.try_exists()?) || !p.is_dir() {
            let err: String = format!("path not found or not a directory: {}", p.display());
            return Err(Error::new(ErrorKind::NotFound, err.as_str()));
        }
        Ok(())
    };

    exists(p)?;

    let lib_path: PathBuf = p.join(&LIB_DIR);
    let inc_path: PathBuf = p.join(&INC_DIR);

    exists(lib_path.as_path())?;
    exists(inc_path.as_path())?;

    Ok((lib_path, inc_path))
}

fn main() {
    let out_dir_s: String = env::var(OUT_DIR_VAR).unwrap();
    let out_dir: &Path = Path::new(&out_dir_s);

    println!("cargo:rerun-if-env-changed={}", XDP_PATH_VAR);
    println!("cargo:rerun-if-changed={}", WRAPPER_HEADER_NAME);
    println!("cargo:rerun-if-changed={}", INLINED_C_NAME);

    let xdp_path: PathBuf = match env::var(XDP_PATH_VAR) {
        Ok(s) => PathBuf::from(s),
        Err(_) => {
            let prog_files_s: String = env::var(PROGRAM_FILES_VAR).unwrap();
            let prog_files: &Path = Path::new(&prog_files_s);
            let xdp_path: PathBuf = prog_files.join("xdp");
            xdp_path
        }
    };

    let (lib_path, inc_path): (PathBuf, PathBuf) = match resolve_xdp_path(xdp_path.as_path()) {
        Ok(t) => t,
        Err(e) => panic!("Failed to resolve XDP installation: {}", e.to_string())
    };

    // Point cargo to libraries.
    println!("cargo:rustc-link-search=native={}", lib_path.display());
    println!("cargo:rustc-link-lib={}", XDP_API_LIB);

    let mut builder = Builder::default();
    for t in TYPE_BLOCKLIST.iter() {
        builder = builder.blocklist_type(t);
    }
    // Generate bindings for headers.
    let bindings: Bindings = builder
        .clang_arg(&format!("-I{}", inc_path.display()))
        .clang_arg("-mavx")
        .header(WRAPPER_HEADER_NAME)
        // NB SAL defines still get included despite having no functional impact.
        .blocklist_item(SAL_BLOCKLIST_REGEX)
        .allowlist_file(FILE_ALLOWLIST_REGEX)
        // Allow the inline function wrappers to be generated.
        .allowlist_file(format!(".*{}", WRAPPER_HEADER_NAME))
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
    builder.file(INLINED_C_NAME);
    builder.include(inc_path);
    builder.compile("inlined");
}
