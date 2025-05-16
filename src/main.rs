use std::{collections::HashMap, fs, path::Path, iter::Peekable, str::Chars, sync::mpsc, time::Duration};
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use indexmap::IndexMap;
use reqwest;
use std::error::Error as StdError;
use futures::future::BoxFuture;
use notify::{Watcher, RecursiveMode, recommended_watcher};
use chrono;

// === TYPES ===

#[derive(Debug, Clone, PartialEq)]
enum ExprAst {
    Symbol(String),
    Number(i64),
    List(Vec<ExprAst>),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(i64),
    String(String),
    Json(JsonValue),
}

#[derive(Debug)]
pub enum Error {
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

#[derive(Debug, Default)]
pub struct ValueCache {
    values: HashMap<String, Value>,
    last_update: HashMap<String, String>,
}

impl ValueCache {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            last_update: HashMap::new(),
        }
    }
    
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
    
    pub fn insert(&mut self, key: String, value: Value) {
        let now = chrono::Utc::now().to_rfc3339();
        self.last_update.insert(key.clone(), now);
        self.values.insert(key, value);
    }
    
    pub fn save_to_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.values)?;
        fs::write(path, json)?;
        Ok(())
    }
    
    pub fn load_from_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(());
        }
        
        let json = fs::read_to_string(path)?;
        self.values = serde_json::from_str(&json)?;
        
        let now = chrono::Utc::now().to_rfc3339();
        for key in self.values.keys() {
            self.last_update.insert(key.clone(), now.clone());
        }
        Ok(())
    }
}

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
pub fn eval<'a>(ast: &'a ExprAst, context: &'a mut IndexMap<String, Value>) -> BoxFuture<'a, Result<Value, Error>> {
    Box::pin(async move {
        match ast {
            ExprAst::Symbol(s) => Ok(context
                .get(s)
                .cloned()
                .ok_or_else(|| Error::EvalError(format!("Undefined symbol: {}", s)))?),
            ExprAst::Number(n) => Ok(Value::Number(*n)),
            ExprAst::String(s) => Ok(Value::String(s.clone())),
            ExprAst::List(list) => {
                if list.is_empty() {
                    return Err(Error::EvalError("Cannot evaluate empty list".to_string()));
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
                                let value = eval(value_node, context).await?;
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
                                let val = eval(arg_node, context).await?;
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
                                let val = eval(arg_node, context).await?;
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
                            match eval(&args[0], context).await? {
                                Value::String(url) => {
                                    let body = reqwest::get(&url).await?.text().await?;
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
                            match eval(&args[0], context).await? {
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
                            let json_arg = eval(&args[0], context).await?;
                            let key_arg = eval(&args[1], context).await?;

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
                            match eval(&args[0], context).await? {
                                Value::String(s) => {
                                    Ok(Value::String(s.to_uppercase()))
                                }
                                _ => Err(Error::EvalError(
                                    "'str.upper' expects a string argument".into(),
                                )),
                            }
                        }
                        _ => {
                            Err(Error::EvalError(format!(
                                "Unknown function symbol '{}' encountered, returning nil.",
                                op
                            )))
                        }
                    }
                } else {
                    Err(Error::EvalError(format!(
                        "List head must be a function/operator symbol, got: {:?}",
                        op_node
                    )))
                }
            }
        }
    })
}

// === MAIN ===

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: garden <file.expr>");
        return Ok(());
    }
    
    let file_path = Path::new(&args[1]);
    let cache_path = file_path.with_extension("expr.value");
    
    // Initialize the value cache
    let mut value_cache = ValueCache::new();
    
    // Try to load previous values
    if let Err(e) = value_cache.load_from_file(&cache_path) {
        eprintln!("Warning: Could not load cached values: {}", e);
    }
    
    // Create a channel to receive file change events
    let (tx, rx) = mpsc::channel();
    
    // Create a file watcher
    let mut watcher = recommended_watcher(tx)?;
    
    // Watch the target file
    watcher.watch(file_path, RecursiveMode::NonRecursive)?;
    
    println!("Garden is watching {}...", file_path.display());
    println!("(Press Ctrl+C to exit)");
    
    // Create a context from ValueCache for evaluation
    let mut context: IndexMap<String, Value> = IndexMap::new();
    
    // Initial run
    if let Err(e) = run_once(file_path, &mut context).await {
        eprintln!("Error: {}", e);
    }
    
    // Save results to cache
    for (key, value) in &context {
        value_cache.insert(key.clone(), value.clone());
    }
    if let Err(e) = value_cache.save_to_file(&cache_path) {
        eprintln!("Warning: Could not save cached values: {}", e);
    }
    
    // Event loop
    for res in rx {
        match res {
            Ok(_) => {
                if let Err(e) = run_once(file_path, &mut context).await {
                    eprintln!("Error: {}", e);
                } else {
                    // Update cache and save after successful run
                    for (key, value) in &context {
                        value_cache.insert(key.clone(), value.clone());
                    }
                    if let Err(e) = value_cache.save_to_file(&cache_path) {
                        eprintln!("Warning: Could not save cached values: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }
    
    Ok(())
}

async fn run_once(path: &Path, context: &mut IndexMap<String, Value>) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nRevaluating expressions in {}...", path.display());
    
    let src = fs::read_to_string(path)?;
    let ast_nodes = parse(&src)?;
    
    // Get the source lines for better display
    let source_lines: Vec<&str> = src.lines().collect();
    
    for (i, node) in ast_nodes.iter().enumerate() {
        let result = eval(node, context).await?;
        
        // For multiline expressions, this is simplified
        let line_num = i + 1;
        let source = if i < source_lines.len() {
            source_lines[i]
        } else {
            "<unknown source>"
        };
        
        // Format the output with colors (using ANSI escape codes)
        // Clear the line, print source, then append value with green color
        println!("\x1B[2K\x1B[0;1m{:>3}|\x1B[0m {} \x1B[0;32m=> {:?}\x1B[0m", 
                 line_num, source, result);
    }
    
    Ok(())
}

pub fn convert_json_value(json_val: JsonValue) -> Result<Value, Error> {
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

pub async fn evaluate_file(file_path: &Path) -> Result<(IndexMap<String, Value>, Option<Value>), Box<dyn std::error::Error>> {
    let input = fs::read_to_string(file_path)?;
    let ast_nodes = parse(&input)?;

    let mut context: IndexMap<String, Value> = IndexMap::new();
    let mut last_result: Option<Value> = None;

    for node in ast_nodes {
        let value = eval(&node, &mut context).await?;
        last_result = Some(value);
    }

    Ok((context, last_result))
}

pub async fn evaluate_form(code: &str, context: &mut IndexMap<String, Value>) -> Result<Value, Error> {
    let ast_nodes = parse(code)?;
    let mut last_result: Option<Value> = None;

    for node in ast_nodes {
        let value = eval(&node, context).await?;
        last_result = Some(value);
    }

    last_result.ok_or_else(|| Error::EvalError("No result found".to_string()))
}


