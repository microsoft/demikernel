// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use cfgrammar::yacc::YaccKind;
use lrlex::CTLexerBuilder;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

fn main() -> Result<()> {
    CTLexerBuilder::new()
        .lrpar_config(|ctp| {
            ctp.yacckind(YaccKind::Grmtools)
                .grammar_in_src_dir("grammar.y")
                .unwrap()
        })
        .lexer_in_src_dir("tokens.l")
        .expect("failed to load lexical rules")
        .build()
        .expect("failed to build lexer");

    Ok(())
}
