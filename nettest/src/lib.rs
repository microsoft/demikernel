// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Modules
//======================================================================================================================

pub mod glue;

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use glue::Event;
use lrlex::{DefaultLexerTypes, LRNonStreamingLexer, LRNonStreamingLexerDef, LexerDef};
use lrpar::{LexError, LexParseError, Lexeme, Lexer, NonStreamingLexer};

lrlex::lrlex_mod!("tokens.l");
lrpar::lrpar_mod!("grammar.y");

//======================================================================================================================
// Standalone Functions
//======================================================================================================================
pub fn run_lexer(line: &str, verbose: bool) -> Result<()> {
    let lexerdef: LRNonStreamingLexerDef<DefaultLexerTypes> = tokens_l::lexerdef();
    let lexer: LRNonStreamingLexer<'_, '_, DefaultLexerTypes> = lexerdef.lexer(line);
    for lexeme in lexer.iter() {
        match lexeme {
            // Succeeded to match a known lexeme.
            Ok(lexeme) => {
                if verbose {
                    let text: &str = lexer.span_str(lexeme.span());
                    let id: u32 = lexeme.tok_id();
                    let name: &str = lexerdef
                        .get_rule_by_id(id)
                        .name
                        .as_ref()
                        .expect("lexical rules should have names");
                    println!("Lexeme: name={:?}, text={:?}", name, text);
                }
            },
            // Failed match a known lexeme.
            Err(e) => {
                let start: usize = e.span().start();
                let end: usize = e.span().end();
                let token: &str = &line[start..end];
                let cause: String = format!("Unknown Lexeme (start={:?}, end={:?}, token={:?})", start, end, token);
                eprintln!("Lexical Error: {:?}", cause);
                anyhow::bail!(cause);
            },
        }
    }

    Ok(())
}

pub fn run_parser(line: &str, verbose: bool) -> Result<Option<Event>> {
    let lexerdef: LRNonStreamingLexerDef<DefaultLexerTypes> = tokens_l::lexerdef();
    let lexer: LRNonStreamingLexer<'_, '_, DefaultLexerTypes> = lexerdef.lexer(&line);
    let (result, errs): (_, Vec<LexParseError<u32, DefaultLexerTypes>>) = grammar_y::parse(&lexer);
    if !errs.is_empty() {
        for e in errs {
            let cause: String = format!("{:?}", e.pp(&lexer, &grammar_y::token_epp));
            println!("Syntax Error: {:?}", cause);
            anyhow::bail!(cause)
        }
    }

    if verbose {
        println!("Syntax Analysis: passed");
    }

    Ok(result.unwrap())
}
