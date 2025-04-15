use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use ron::ser::to_string_pretty;
use ron::ser::PrettyConfig;
use std::iter::Peekable;
use std::str::Chars;

// === TYPES ===

#[derive(Debug, Clone, PartialEq)]
enum ExprAst {
    Symbol(String),
    Number(i64),
    List(Vec<ExprAst>),
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Number(i64),
}

#[derive(Debug)]
enum Error {
    ParseError(String),
    EvalError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ParseError(msg) => write!(f, "Parse Error: {}", msg),
            Error::EvalError(msg) => write!(f, "Evaluation Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

// === HOST FUNCTIONS ===

fn http_get(url: &str) -> String {
    let body = reqwest::blocking::get(url)
        .expect("Failed GET")
        .text()
        .expect("Failed to read response body");
    body
}

// === Parser ===

fn parse_expr(tokens: &mut Peekable<Chars>) -> Result<ExprAst, Error> {
    match tokens.peek() {
        Some(&'(') => {
            tokens.next();
            let mut list = Vec::new();
            while tokens.peek() != Some(&')') {
                while tokens.peek().map_or(false, |c| c.is_whitespace()) {
                    tokens.next();
                }
                if tokens.peek() == Some(&')') {
                    break;
                }
                if tokens.peek().is_none() {
                    return Err(Error::ParseError("Unexpected end of input, missing ')'".to_string()));
                }
                list.push(parse_expr(tokens)?);
                while tokens.peek().map_or(false, |c| c.is_whitespace()) {
                    tokens.next();
                }
            }
            tokens.next();
            Ok(ExprAst::List(list))
        }
        Some(_) => {
            let mut atom = String::new();
            while let Some(&c) = tokens.peek() {
                if c.is_whitespace() || c == '(' || c == ')' {
                    break;
                }
                atom.push(tokens.next().unwrap());
            }

            if atom.is_empty() {
                return Err(Error::ParseError("Expected token, found none".to_string()));
            }

            match atom.parse::<i64>() {
                Ok(n) => Ok(ExprAst::Number(n)),
                Err(_) => Ok(ExprAst::Symbol(atom)),
            }
        }
        None => Err(Error::ParseError("Unexpected end of input".to_string())),
    }
}

fn parse(input: &str) -> Result<Vec<ExprAst>, Error> {
    let mut tokens = input.chars().peekable();
    let mut ast_nodes = Vec::new();

    while tokens.peek().is_some() {
        while tokens.peek().map_or(false, |c| c.is_whitespace()) {
            tokens.next();
        }
        if tokens.peek().is_none() {
            break;
        }
        ast_nodes.push(parse_expr(&mut tokens)?);
    }

    Ok(ast_nodes)
}

// === Evaluator ===
fn eval(ast: &ExprAst, context: &mut HashMap<String, Value>) -> Result<Value, Error> {
    match ast {
        ExprAst::Symbol(s) => context
            .get(s)
            .cloned()
            .ok_or_else(|| Error::EvalError(format!("Undefined symbol: {}", s))),
        ExprAst::Number(n) => Ok(Value::Number(*n)),
        ExprAst::List(list) => {
            if list.is_empty() {
                // Handle empty list if needed, maybe return Nil or error
                return Err(Error::EvalError("Cannot evaluate empty list".to_string()));
            }

            // First element should be the operator/function
            let op_node = &list[0];
            let args = &list[1..];

            match op_node {
                ExprAst::Symbol(op) => {
                    match op.as_str() {
                        "def" => {
                            if args.len() != 2 {
                                return Err(Error::EvalError(format!(
                                    "'def' expects 2 arguments, got {}",
                                    args.len()
                                )));
                            }
                            let var_name_node = &args[0];
                            let value_node = &args[1];

                            if let ExprAst::Symbol(var_name) = var_name_node {
                                let value = eval(value_node, context)?;
                                context.insert(var_name.clone(), value.clone());
                                Ok(value) // 'def' evaluates to the assigned value
                            } else {
                                Err(Error::EvalError(
                                    "'def' first argument must be a symbol".to_string(),
                                ))
                            }
                        }
                        "+" => {
                            let mut sum = 0;
                            for arg_node in args {
                                let val = eval(arg_node, context)?;
                                match val {
                                    Value::Number(n) => sum += n,
                                    // Add type error handling if other Value variants exist
                                     _ => return Err(Error::EvalError("'+' requires number arguments".to_string())),
                                }
                            }
                            Ok(Value::Number(sum))
                        }
                        "*" => {
                            let mut product = 1;
                            for arg_node in args {
                                let val = eval(arg_node, context)?;
                                match val {
                                    Value::Number(n) => product *= n,
                                     // Add type error handling if other Value variants exist
                                    _ => return Err(Error::EvalError("'*' requires number arguments".to_string())),
                                }
                            }
                            Ok(Value::Number(product))
                        }
                        _ => Err(Error::EvalError(format!("Unknown operator or function: {}", op))),
                    }
                }
                // Allow evaluating lists where the head evaluates to a function later?
                // For now, require a symbol.
                 _ => Err(Error::EvalError(
                     "Operator must be a symbol".to_string(),
                 )),
            }
        }
    }
}

// === MAIN ===

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read the expression file
    let file_path = "examples/one.expr"; // Specify the input file
    let input = fs::read_to_string(file_path)?;

    // Parse the input string
    let ast_nodes = parse(&input)?;

    // Create the evaluation context
    let mut context: HashMap<String, Value> = HashMap::new();
    let mut last_result: Option<Value> = None;

    // Evaluate each top-level expression
    for node in ast_nodes {
        match eval(&node, &mut context) {
            Ok(value) => {
                // Optional: Print intermediate results for debugging
                // println!("Evaluating: {:?} -> Result: {:?}", node, value);
                last_result = Some(value);
            }
            Err(e) => {
                eprintln!("Evaluation Error: {}", e); // Use Display impl for Error
                return Err(Box::new(e)); // Stop on error
            }
        }
    }

    // Print the final result (result of the last expression)
    if let Some(result) = last_result {
        println!("Final Result: {:?}", result);
    } else {
        println!("No expressions were evaluated or the file was empty.");
    }

    // Removed old logic using garden_program, expr.path, expr.eval
    // Removed RON serialization logic

    Ok(()) // Indicate successful execution
}

