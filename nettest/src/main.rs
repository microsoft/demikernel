// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use std::{
    fs::File,
    io::{BufRead, BufReader},
};

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

fn main() -> Result<()> {
    // Merge all lines
    let matches: ArgMatches = Command::new("app")
        .author("Pedro Henrique Penna <ppenna@microsoft.com>")
        .about("Network Testing Tool for Demikernel")
        .arg(
            Arg::new("src")
                .long("src")
                .value_parser(clap::value_parser!(String))
                .required(true)
                .value_name("filename")
                .help("Source file"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .value_parser(clap::value_parser!(bool))
                .required(true)
                .help("Verbose output?"),
        )
        .get_matches();

    let verbose: bool = *matches.get_one::<bool>("verbose").expect("missing verbose flag");
    let filename: &String = matches.get_one::<String>("src").expect("missing filename");

    let lines: Vec<String> = read_input_file(filename)?;

    // Process all lines of the source file.
    for line in lines {
        if verbose {
            println!("Line: {:?}", line);
        }
        // Process in multiple passes.
        for pass in ["Lexical Analysis", "Syntax Analysis"] {
            match pass {
                "Lexical Analysis" => match nettest::run_lexer(&line, verbose) {
                    Ok(()) => (),
                    Err(_) => println!("Lexical Analysis: FAILED"),
                },
                "Syntax Analysis" => match nettest::run_parser(&line, verbose) {
                    Ok(result) => println!("{:?}", result),
                    Err(_) => println!("Syntax Analysis: FAILED"),
                },
                _ => unreachable!(),
            }
        }
    }

    println!("Compilation Succeeded");
    Ok(())
}

fn read_input_file(filename: &str) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    let file: File = File::open(filename)?;
    let reader: BufReader<File> = BufReader::new(file);

    // Read all lines of the input file.
    for line in reader.lines() {
        if let Ok(line) = line {
            lines.push(line);
        }
    }

    Ok(lines)
}
