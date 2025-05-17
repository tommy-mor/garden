use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;
use std::str::FromStr;

use crate::{ExprAst, SourceSpan, Error};

#[derive(Parser)]
#[grammar = "expr.pest"]
pub struct ExprParser;

pub fn parse(input: &str) -> Result<Vec<ExprAst>, Error> {
    let pairs = ExprParser::parse(Rule::program, input)
        .map_err(|e| Error::ParseError(e.to_string()))?;
    
    let mut ast_nodes = Vec::new();
    
    // Process each top-level expression in the program
    for pair in pairs {
        match pair.as_rule() {
            Rule::program => {
                // Process all expressions in the program
                for expr_pair in pair.into_inner() {
                    if expr_pair.as_rule() == Rule::EOI {
                        continue; // Skip end of input marker
                    }
                    let expr = parse_expr(expr_pair)?;
                    ast_nodes.push(expr);
                }
            }
            _ => {} // Skip other rules at the top level
        }
    }
    
    Ok(ast_nodes)
}

fn parse_expr(pair: Pair<Rule>) -> Result<ExprAst, Error> {
    // Get source position for span information
    let span = SourceSpan {
        line: pair.line_col().0, // 1-indexed line number from pest
    };
    
    match pair.as_rule() {
        Rule::symbol => {
            let symbol_name = pair.as_str().to_string();
            Ok(ExprAst::Symbol(symbol_name, span))
        }
        Rule::number => {
            let num_str = pair.as_str();
            match i64::from_str(num_str) {
                Ok(n) => Ok(ExprAst::Number(n, span)),
                Err(_) => Err(Error::ParseError(format!("Invalid number: {}", num_str))),
            }
        }
        Rule::string => {
            // Process the string, handling escapes
            // The string token includes the outer quotes
            
            // Find the inner_string rule
            let inner = pair.into_inner().next()
                .ok_or_else(|| Error::ParseError("Expected inner string content".to_string()))?;
            
            // Process escape sequences
            let unescaped = process_string_content(inner.as_str())?;
            Ok(ExprAst::String(unescaped, span))
        }
        Rule::list => {
            let inner_rules = pair.into_inner();
            let mut elements = Vec::new();
            
            for inner_pair in inner_rules {
                let element = parse_expr(inner_pair)?;
                elements.push(element);
            }
            
            Ok(ExprAst::List(elements, span))
        }
        _ => Err(Error::ParseError(format!("Unexpected rule: {:?}", pair.as_rule()))),
    }
}

fn process_string_content(content: &str) -> Result<String, Error> {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars();
    
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some(other) => return Err(Error::ParseError(format!("Invalid escape sequence: \\{}", other))),
                None => return Err(Error::ParseError("Incomplete escape sequence".to_string())),
            }
        } else {
            result.push(c);
        }
    }
    
    Ok(result)
} 