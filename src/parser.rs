use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{Error, SourceSpan, Node, NodeKind};

// Define the grammar using pest's procedural macro
#[derive(Parser)]
#[grammar = "expr.pest"]
pub struct ExprParser;

// Main parsing function that returns a vector of Nodes
pub fn parse(source: &str) -> Result<Vec<Rc<Node>>, Error> {
    // Parse the input using pest
    let pairs = ExprParser::parse(Rule::program, source)
        .map_err(|e| Error::ParseError(e.to_string()))?;
    
    // Process all top-level expressions into nodes
    let mut nodes = Vec::new();
    
    for pair in pairs {
        match pair.as_rule() {
            Rule::expr => {
                let node = parse_expr(pair, source)?;
                nodes.push(node);
            }
            Rule::EOI => {}, // End of input, ignore
            _ => return Err(Error::ParseError(format!("Unexpected rule: {:?}", pair.as_rule()))),
        }
    }
    
    Ok(nodes)
}

// Parse a single expression
fn parse_expr(pair: Pair<Rule>, source: &str) -> Result<Rc<Node>, Error> {
    let line = pair.line_col().0;
    let span_text = pair.as_str().to_string();
    
    // Create basic metadata for the node
    let mut metadata = HashMap::new();
    metadata.insert("line".to_string(), line.to_string());
    
    match pair.as_rule() {
        Rule::symbol => {
            let symbol_name = pair.as_str().to_string();
            metadata.insert("source_type".to_string(), "symbol".to_string());
            Ok(Node::new(
                NodeKind::Symbol(symbol_name.clone()),
                span_text,
                Vec::new(),
                metadata
            ))
        },
        Rule::number => {
            let num_str = pair.as_str();
            let num = num_str.parse::<i64>()
                .map_err(|e| Error::ParseError(format!("Failed to parse number: {}", e)))?;
            metadata.insert("source_type".to_string(), "number".to_string());
            Ok(Node::new(
                NodeKind::Number(num),
                span_text,
                Vec::new(),
                metadata
            ))
        },
        Rule::string => {
            // Remove the quotes from the string literal
            let s = pair.as_str();
            let content = if s.len() >= 2 {
                s[1..s.len()-1].to_string()
            } else {
                return Err(Error::ParseError("Malformed string literal".to_string()));
            };
            metadata.insert("source_type".to_string(), "string".to_string());
            Ok(Node::new(
                NodeKind::String(content),
                span_text,
                Vec::new(),
                metadata
            ))
        },
        Rule::list => {
            let original_text = pair.as_str().to_string();
            
            // Parse inner expressions of the list
            let mut children = Vec::new();
            for inner_pair in pair.into_inner() {
                if inner_pair.as_rule() == Rule::expr {
                    let child_node = parse_expr(inner_pair, source)?;
                    children.push(child_node);
                }
            }
            
            if children.is_empty() {
                metadata.insert("source_type".to_string(), "empty_list".to_string());
                return Ok(Node::new(
                    NodeKind::List,
                    original_text,
                    Vec::new(),
                    metadata
                ));
            }
            
            // Check if the first element is a symbol to determine the operation type
            if let Some(first_child) = children.first() {
                if let NodeKind::Symbol(op) = &first_child.kind {
                    let node_kind = match op.as_str() {
                        "def" => {
                            metadata.insert("source_type".to_string(), "definition".to_string());
                            NodeKind::Definition
                        },
                        "let" => {
                            metadata.insert("source_type".to_string(), "let".to_string());
                            NodeKind::Let
                        },
                        "+" => {
                            metadata.insert("source_type".to_string(), "addition".to_string());
                            NodeKind::Addition
                        },
                        "*" => {
                            metadata.insert("source_type".to_string(), "multiplication".to_string());
                            NodeKind::Multiplication
                        },
                        "http.get" => {
                            metadata.insert("source_type".to_string(), "http_get".to_string());
                            NodeKind::HttpGet
                        },
                        "json.parse" => {
                            metadata.insert("source_type".to_string(), "json_parse".to_string());
                            NodeKind::JsonParse
                        },
                        "get" => {
                            metadata.insert("source_type".to_string(), "json_get".to_string());
                            NodeKind::JsonGet
                        },
                        "str.upper" => {
                            metadata.insert("source_type".to_string(), "string_upper".to_string());
                            NodeKind::StringUpper
                        },
                        _ => {
                            metadata.insert("source_type".to_string(), "function_call".to_string());
                            metadata.insert("function_name".to_string(), op.clone());
                            NodeKind::List
                        }
                    };
                    
                    return Ok(Node::new(node_kind, original_text, children, metadata));
                }
            }
            
            // Generic list
            metadata.insert("source_type".to_string(), "list".to_string());
            Ok(Node::new(NodeKind::List, original_text, children, metadata))
        },
        Rule::expr => {
            // Recursively process a nested expression
            let inner = pair.into_inner().next()
                .ok_or_else(|| Error::ParseError("Empty expression".to_string()))?;
            parse_expr(inner, source)
        },
        _ => Err(Error::ParseError(format!("Unexpected rule: {:?}", pair.as_rule()))),
    }
} 