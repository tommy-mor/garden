use std::{collections::HashMap, fs, path::Path, iter::Peekable, str::Chars};
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use indexmap::IndexMap;
use reqwest;

// === TYPES ===

#[derive(Debug, Clone, PartialEq)]
enum ExprAst {
    Symbol(String),
    Number(i64),
    List(Vec<ExprAst>),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(i64),
    String(String),
    Json(JsonValue),
}

#[derive(Debug)]
enum Error {
    ParseError(String),
    EvalError(String),
    HttpError(String),
    JsonError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ParseError(msg) => write!(f, "Parse Error: {}", msg),
            Error::EvalError(msg) => write!(f, "Evaluation Error: {}", msg),
            Error::HttpError(msg) => write!(f, "HTTP Error: {}", msg),
            Error::JsonError(msg) => write!(f, "JSON Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

// === Parser ===

fn parse_expr(tokens: &mut Peekable<Chars>) -> Result<ExprAst, Error> {
    while tokens.peek().map_or(false, |c| c.is_whitespace()) {
        tokens.next();
    }

    match tokens.peek() {
        Some(&'(') => {
            tokens.next();
            let mut list = Vec::new();
            loop {
                while tokens.peek().map_or(false, |c| c.is_whitespace()) {
                    tokens.next();
                }
                match tokens.peek() {
                    Some(&')') => {
                        tokens.next();
                        break Ok(ExprAst::List(list));
                    }
                    Some(_) => {
                        list.push(parse_expr(tokens)?);
                    }
                    None => {
                        break Err(Error::ParseError(
                            "Unexpected end of input, missing ')'".to_string(),
                        ));
                    }
                }
            }
        }
        Some(&'"') => {
            tokens.next();
            let mut string_content = String::new();
            while let Some(&c) = tokens.peek() {
                if c == '"' {
                    tokens.next();
                    return Ok(ExprAst::String(string_content));
                }
                if c == '\\' {
                    tokens.next();
                    if let Some(escaped_char) = tokens.next() {
                        match escaped_char {
                            'n' => string_content.push('\n'),
                            't' => string_content.push('\t'),
                            '\\' => string_content.push('\\'),
                            '"' => string_content.push('"'),
                            _ => return Err(Error::ParseError(format!("Invalid escape sequence: \\{}", escaped_char))),
                        }
                    } else {
                        return Err(Error::ParseError("Unexpected end of input after escape character".to_string()));
                    }
                } else {
                    string_content.push(tokens.next().unwrap());
                }
            }
            Err(Error::ParseError("Unexpected end of input, unclosed string literal".to_string()))
        }
        Some(_) => {
            let mut atom = String::new();
            while let Some(&c) = tokens.peek() {
                if c.is_whitespace() || "()\"".contains(c) {
                    break;
                }
                atom.push(tokens.next().unwrap());
            }

            if atom.is_empty() {
                return Err(Error::ParseError(
                    "Expected token, found none or only whitespace".to_string(),
                ));
            }

            match atom.parse::<i64>() {
                Ok(n) => Ok(ExprAst::Number(n)),
                Err(_) => Ok(ExprAst::Symbol(atom)),
            }
        }
        None => Err(Error::ParseError(
            "Unexpected end of input".to_string(),
        )),
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
fn eval(ast: &ExprAst, context: &mut IndexMap<String, Value>) -> Result<Value, Error> {
    match ast {
        ExprAst::Symbol(s) => context
            .get(s)
            .cloned()
            .ok_or_else(|| Error::EvalError(format!("Undefined symbol: {}", s))),
        ExprAst::Number(n) => Ok(Value::Number(*n)),
        ExprAst::String(s) => Ok(Value::String(s.clone())),
        ExprAst::List(list) => {
            if list.is_empty() {
                return Err(Error::EvalError(
                    "Cannot evaluate empty list".to_string(),
                ));
            }

            let op_node = &list[0];
            let args = &list[1..];

            if let ExprAst::Symbol(op) = op_node {
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
                            Ok(value)
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
                                _ => return Err(Error::EvalError(
                                    "'+' requires number arguments".to_string(),
                                )),
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
                                _ => return Err(Error::EvalError(
                                    "'*' requires number arguments".to_string(),
                                )),
                            }
                        }
                        Ok(Value::Number(product))
                    }
                    "http.get" => {
                        if args.len() != 1 {
                            return Err(Error::EvalError(
                                "'http.get' expects 1 argument (url)".into(),
                            ));
                        }
                        match eval(&args[0], context)? {
                            Value::String(url) => {
                                let body = reqwest::blocking::get(&url)?.text()?;
                                Ok(Value::String(body))
                            }
                            _ => Err(Error::EvalError(
                                "'http.get' expects a string argument".into(),
                            )),
                        }
                    }
                    "json.parse" => {
                        if args.len() != 1 {
                            return Err(Error::EvalError(
                                "'json.parse' expects 1 argument (string)".into(),
                            ));
                        }
                        match eval(&args[0], context)? {
                            Value::String(s) => {
                                let json_data: JsonValue = serde_json::from_str(&s)?;
                                Ok(Value::Json(json_data))
                            }
                            _ => Err(Error::EvalError(
                                "'json.parse' expects a string argument".into(),
                            )),
                        }
                    }
                    "get" => {
                        if args.len() != 2 {
                            return Err(Error::EvalError(
                                "'get' expects 2 arguments (json, key)".into(),
                            ));
                        }
                        let json_arg = eval(&args[0], context)?;
                        let key_arg = eval(&args[1], context)?;

                        match (&json_arg, &key_arg) {
                            (Value::Json(json), Value::String(key)) => {
                                match json.get(key) {
                                    Some(v) => convert_json_value(v.clone()),
                                    None => Err(Error::EvalError(format!(
                                        "Key '{}' not found in JSON object",
                                        key
                                    ))),
                                }
                            }
                            _ => Err(Error::EvalError(format!(
                                "'get' expects (json, string) arguments, got ({:?}, {:?})",
                                &json_arg, &key_arg
                            ))),
                        }
                    }
                    "str.upper" => {
                        if args.len() != 1 {
                            return Err(Error::EvalError(
                                "'str.upper' expects 1 argument (string)".into(),
                            ));
                        }
                        match eval(&args[0], context)? {
                            Value::String(s) => {
                                Ok(Value::String(s.to_uppercase()))
                            }
                            _ => Err(Error::EvalError(
                                "'str.upper' expects a string argument".into(),
                            )),
                        }
                    }
                    _ => Err(Error::EvalError(format!(
                        "Unknown operator or function symbol: {}",
                        op
                    ))),
                }
            } else {
                Err(Error::EvalError(format!(
                    "List head must be a function/operator symbol, got: {:?}",
                    op_node
                )))
            }
        }
    }
}

// === MAIN ===

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Define the file to watch (can be made configurable later)
    let file_to_watch = Path::new("examples/http.expr");

    Ok(())
}

fn convert_json_value(json_val: JsonValue) -> Result<Value, Error> {
    match json_val {
        JsonValue::String(s) => Ok(Value::String(s)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Number(i))
            } else {
                Err(Error::EvalError(format!(
                    "Unsupported number type from JSON: {}",
                    n
                )))
            }
        }
        JsonValue::Bool(b) => Err(Error::EvalError(format!(
            "Boolean JSON value ({}) not yet supported as primitive",
            b
        ))),
        JsonValue::Null => Err(Error::EvalError(
            "Null JSON value not yet supported as primitive".to_string(),
        )),
        JsonValue::Array(_) => Err(Error::EvalError(
            "Array JSON value not yet supported as primitive".to_string(),
        )),
        JsonValue::Object(_) => Err(Error::EvalError(
            "Nested JSON objects not directly supported as primitive values".to_string(),
        )),
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::HttpError(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::JsonError(err.to_string())
    }
}

pub fn evaluate_file(file_path: &Path) -> Result<(IndexMap<String, Value>, Option<Value>), Box<dyn std::error::Error>> {
    let input = fs::read_to_string(file_path)?;
    let ast_nodes = parse(&input)?;

    let mut context: IndexMap<String, Value> = IndexMap::new();
    let mut last_result: Option<Value> = None;

    for node in ast_nodes {
        let value = eval(&node, &mut context)?;
        last_result = Some(value);
    }

    Ok((context, last_result))
}


