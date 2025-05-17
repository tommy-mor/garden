use std::{collections::HashMap, fs, path::Path, iter::Peekable, str::Chars, sync::mpsc, time::Duration};
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use indexmap::IndexMap;
use reqwest;
use std::error::Error as StdError;
use futures::future::{BoxFuture, Future};
use std::pin::Pin;
use notify::{Watcher, RecursiveMode, recommended_watcher};
use chrono;
use std::rc::Rc;
use std::hash::{Hash, Hasher};
use blake3;
use hex;

// Add pest parser module
mod parser;

// === TYPES ===

#[derive(Debug, Clone, Copy, PartialEq)]
struct SourceSpan {
    line: usize,
    // column: usize, // TODO: Add column later
}

#[derive(Debug, Clone, PartialEq)]
enum ExprAst {
    Symbol(String, SourceSpan),
    Number(i64, SourceSpan),
    List(Vec<ExprAst>, SourceSpan),
    String(String, SourceSpan),
}

// Node ID based on content hash
type NodeId = [u8; 32]; // 32 bytes for BLAKE3 hash

// Kind of node for evaluation purposes
#[derive(Debug, Clone, PartialEq)]
enum NodeKind {
    Symbol(String),
    Number(i64),
    String(String),
    List,
    // More specific operations could be added here
    Definition,
    Addition,
    Multiplication,
    HttpGet,
    JsonParse,
    JsonGet,
    StringUpper,
}

// Immutable computation tree node
#[derive(Debug, Clone)]
pub struct Node {
    id: NodeId,                      // Content-based hash for identity
    kind: NodeKind,                  // The kind of operation this node represents
    code_snippet: String,            // Original source code
    children: Vec<Rc<Node>>,         // Child nodes - immutable references
    cached_value: Option<Result<Value, Error>>, // Last evaluated result
    metadata: HashMap<String, String>, // Source location, timestamps, etc.
}

impl Node {
    // Create a new node and compute its hash
    pub fn new(
        kind: NodeKind,
        code_snippet: String,
        children: Vec<Rc<Node>>,
        metadata: HashMap<String, String>,
    ) -> Rc<Self> {
        // Compute hash based on kind, code, and children
        let id = Self::compute_hash(&kind, &code_snippet, &children);
        
        Rc::new(Self {
            id,
            kind,
            code_snippet,
            children,
            cached_value: None,
            metadata,
        })
    }
    
    // Compute a structural hash based on the node's content and its children
    fn compute_hash(kind: &NodeKind, code: &str, children: &[Rc<Node>]) -> NodeId {
        let mut hasher = blake3::Hasher::new();
        
        // Add kind discriminator
        match kind {
            NodeKind::Symbol(s) => {
                hasher.update(b"Symbol:");
                hasher.update(s.as_bytes());
            }
            NodeKind::Number(n) => {
                hasher.update(b"Number:");
                hasher.update(&n.to_le_bytes());
            }
            NodeKind::String(s) => {
                hasher.update(b"String:");
                hasher.update(s.as_bytes());
            }
            NodeKind::List => {
                hasher.update(b"List");
            }
            NodeKind::Definition => {
                hasher.update(b"Definition");
            }
            NodeKind::Addition => {
                hasher.update(b"Addition");
            }
            NodeKind::Multiplication => {
                hasher.update(b"Multiplication");
            }
            NodeKind::HttpGet => {
                hasher.update(b"HttpGet");
            }
            NodeKind::JsonParse => {
                hasher.update(b"JsonParse");
            }
            NodeKind::JsonGet => {
                hasher.update(b"JsonGet");
            }
            NodeKind::StringUpper => {
                hasher.update(b"StringUpper");
            }
        }
        
        // Add code snippet
        hasher.update(code.as_bytes());
        
        // Add children's hashes
        for child in children {
            hasher.update(&child.id);
        }
        
        // Finalize hash
        *hasher.finalize().as_bytes()
    }
    
    // Get the node's ID
    pub fn id(&self) -> &NodeId {
        &self.id
    }
    
    // Get cached value if any
    pub fn cached_value(&self) -> Option<&Result<Value, Error>> {
        self.cached_value.as_ref()
    }
    
    // Set cached value
    pub fn with_cached_value(mut self, value: Result<Value, Error>) -> Self {
        self.cached_value = Some(value);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(i64),
    String(String),
    Json(JsonValue),
}

#[derive(Debug, Clone)]
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
// Use the new pest-based parser module instead of the old parser functions

// === Evaluator ===
pub fn eval<'a>(ast: &'a ExprAst, context: &'a mut IndexMap<String, Value>) -> BoxFuture<'a, Result<Value, Error>> {
    Box::pin(async move {
        match ast {
            ExprAst::Symbol(s, _) => Ok(context
                .get(s)
                .cloned()
                .ok_or_else(|| Error::EvalError(format!("Undefined symbol: {}", s)))?),
            ExprAst::Number(n, _) => Ok(Value::Number(*n)),
            ExprAst::String(s, _) => Ok(Value::String(s.clone())),
            ExprAst::List(list, _list_span) => {
                if list.is_empty() {
                    return Err(Error::EvalError("Cannot evaluate empty list".to_string()));
                }

                let op_node = &list[0];
                let args = &list[1..];

                if let ExprAst::Symbol(op, _op_span) = op_node {
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

                            if let ExprAst::Symbol(var_name, _var_span) = var_name_node {
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
    let value_cache_path = file_path.with_extension("expr.value");
    let node_cache_path = file_path.with_extension("expr.nodecache");
    
    // Initialize the value cache
    let mut value_cache = ValueCache::new();
    
    // Try to load previous values
    if let Err(e) = value_cache.load_from_file(&value_cache_path) {
        eprintln!("Warning: Could not load cached values: {}", e);
    }
    
    // Initialize the node cache and load previous state if available
    let mut node_cache = NodeCache::new();
    if let Err(e) = node_cache.load_from_file(&node_cache_path) {
        eprintln!("Warning: Could not load node cache: {}", e);
    }
    
    // Create a channel to receive file change events
    let (tx, rx) = mpsc::channel();
    
    // Create a file watcher
    let mut watcher = recommended_watcher(tx)?;
    
    // Watch the target file
    watcher.watch(file_path, RecursiveMode::NonRecursive)?;
    
    println!("Garden is watching {}...", file_path.display());
    println!("(Press Ctrl+C to exit)");
    
    // Create a context for evaluation
    let mut context: IndexMap<String, Value> = IndexMap::new();
    
    // Initial run
    if let Err(e) = run_once(file_path, &mut context, &mut node_cache).await {
        eprintln!("Error: {}", e);
    }
    
    // Save caches
    for (key, value) in &context {
        value_cache.insert(key.clone(), value.clone());
    }
    if let Err(e) = value_cache.save_to_file(&value_cache_path) {
        eprintln!("Warning: Could not save cached values: {}", e);
    }
    if let Err(e) = node_cache.save_to_file(&node_cache_path) {
        eprintln!("Warning: Could not save node cache: {}", e);
    }
    
    // Event loop
    for res in rx {
        match res {
            Ok(_) => {
                if let Err(e) = run_once(file_path, &mut context, &mut node_cache).await {
                    eprintln!("Error: {}", e);
                } else {
                    // Update cache and save after successful run
                    for (key, value) in &context {
                        value_cache.insert(key.clone(), value.clone());
                    }
                    if let Err(e) = value_cache.save_to_file(&value_cache_path) {
                        eprintln!("Warning: Could not save cached values: {}", e);
                    }
                    if let Err(e) = node_cache.save_to_file(&node_cache_path) {
                        eprintln!("Warning: Could not save node cache: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }
    
    Ok(())
}

// New struct for display
#[derive(Debug)]
struct DisplayInfo {
    line: usize,
    code_snippet: String,
    id_hex_short: String, // Short version of NodeId hex
    value_str: String,   // String representation of the Value or Error
}

fn collect_display_info_recursive(
    node: &Rc<Node>,
    node_cache: &NodeCache,
    display_items: &mut Vec<DisplayInfo>,
    visited_for_display: &mut std::collections::HashSet<NodeId>
) {
    if !visited_for_display.insert(node.id) { // If already visited, skip
        return;
    }

    // Check if this node was marked as changed during the latest evaluation cycle
    if node_cache.was_changed(&node.id) {
        let line_str = node.metadata.get("line")
            .expect("Node metadata should contain 'line' information after parsing");
        let line = line_str.parse::<usize>()
            .expect("Line metadata should be a parsable usize");

        let id_hex_short = hex::encode(&node.id[0..4]); // First 4 bytes for display

        let value_representation = match node_cache.get(&node.id) {
            Some(Ok(value)) => format!("{:?}", value), // Or a more custom pretty print
            Some(Err(error)) => format!("Error: {}", error),
            None => "Value not found in cache (Error: should not happen for a node marked as changed)".to_string(),
        };

        display_items.push(DisplayInfo {
            line,
            code_snippet: node.code_snippet.clone(),
            id_hex_short,
            value_str: value_representation,
        });
    }

    // Recursively visit children
    for child in &node.children {
        collect_display_info_recursive(child, node_cache, display_items, visited_for_display);
    }
}

async fn run_once(path: &Path, context: &mut IndexMap<String, Value>, node_cache: &mut NodeCache) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nRevaluating expressions in {}...", path.display());
    
    node_cache.prepare_for_evaluation();
    
    let src = fs::read_to_string(path)?;
    // Call the new parser module's parse function
    let ast_nodes = parser::parse(&src)?; // Vec<ExprAst> with SourceSpan
    
    let mut roots = Vec::new();
    for ast_node in &ast_nodes {
        let root_node = ast_to_node_tree(ast_node);
        // Evaluate the node. eval_node uses/updates cache and context.
        // Errors during evaluation are also cached by eval_node.
        if let Err(e) = eval_node(&root_node, context, node_cache).await {
            // Even if eval_node returns an error here, it should have been cached.
            // The display logic below will pick up errors from the cache.
            // However, we might want to log a more immediate, less structured error for top-level failures.
            eprintln!("Note: A top-level expression resulted in an error: {}. Code: {}. It will be listed in changed expressions if its error state is new.", e, root_node.code_snippet);
        }
        roots.push(root_node);
    }
    
    // Collect all changed nodes for display by traversing the graph
    // and checking against node_cache.changed_nodes
    let mut display_items: Vec<DisplayInfo> = Vec::new();
    let mut visited_for_display: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for root_node in &roots {
        collect_display_info_recursive(root_node, node_cache, &mut display_items, &mut visited_for_display);
    }
    
    // Sort by line number for ordered output
    display_items.sort_by_key(|item| item.line);
    
    println!("Changed expressions:");
    if display_items.is_empty() {
        println!("No expressions changed in this evaluation.");
    } else {
        for item in display_items {
            println!("\x1B[2K\x1B[0;1m{:>3}|\x1B[0m {} \x1B[0;36m[{}]\x1B[0m \x1B[0;32m=> {}\x1B[0m", 
                    item.line, item.code_snippet, item.id_hex_short, item.value_str);
        }
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
    // Call the new parser module's parse function
    let ast_nodes = parser::parse(&input)?;

    let mut context: IndexMap<String, Value> = IndexMap::new();
    let mut last_result: Option<Value> = None;

    for node in ast_nodes {
        let value = eval(&node, &mut context).await?;
        last_result = Some(value);
    }

    Ok((context, last_result))
}

pub async fn evaluate_form(code: &str, context: &mut IndexMap<String, Value>) -> Result<Value, Error> {
    // Call the new parser module's parse function
    let ast_nodes = parser::parse(code)?;
    let mut last_result: Option<Value> = None;

    for node in ast_nodes {
        let value = eval(&node, context).await?;
        last_result = Some(value);
    }

    last_result.ok_or_else(|| Error::EvalError("No result found".to_string()))
}

// === Node Evaluation ===

// Convert from ExprAst to Node tree
fn ast_to_node_tree(ast: &ExprAst) -> Rc<Node> {
    let mut metadata = HashMap::new();
    
    // Extract and store line information
    let span: SourceSpan = match ast {
        ExprAst::Symbol(_, s) => *s,
        ExprAst::Number(_, s) => *s,
        ExprAst::List(_, s) => *s,
        ExprAst::String(_, s) => *s,
    };
    metadata.insert("line".to_string(), span.line.to_string());

    match ast {
        ExprAst::Symbol(s, _) => {
            metadata.insert("source_type".to_string(), "symbol".to_string());
            Node::new(
                NodeKind::Symbol(s.clone()),
                s.clone(),
                Vec::new(),
                metadata
            )
        },
        ExprAst::Number(n, _) => {
            metadata.insert("source_type".to_string(), "number".to_string());
            Node::new(
                NodeKind::Number(*n),
                n.to_string(),
                Vec::new(),
                metadata
            )
        },
        ExprAst::String(s, _) => {
            metadata.insert("source_type".to_string(), "string".to_string());
            let code_snippet = format!("\"{}\"", s); // Keep string quoted in snippet
            Node::new(
                NodeKind::String(s.clone()),
                code_snippet,
                Vec::new(),
                metadata
            )
        },
        ExprAst::List(items, _list_span) => {
            if items.is_empty() {
                metadata.insert("source_type".to_string(), "empty_list".to_string());
                return Node::new(
                    NodeKind::List,
                    "()".to_string(),
                    Vec::new(),
                    metadata
                );
            }
            
            // Create child nodes for ALL items including the operator
            let children: Vec<Rc<Node>> = items.iter().map(ast_to_node_tree).collect();
            
            // Determine the operation type from the first item if it's a symbol
            if let ExprAst::Symbol(op, _op_span) = &items[0] {
                let node_kind = match op.as_str() {
                    "def" => {
                        metadata.insert("source_type".to_string(), "definition".to_string());
                        NodeKind::Definition
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
                
                // Reconstruct source code
                let code_snippet = format!(
                    "({})", 
                    items.iter()
                         .map(|item| match item {
                             ExprAst::String(s, _) => format!("\"{}\"", s),
                             _ => format!("{:?}", item)
                                 .split('(').nth(0).unwrap_or("").to_lowercase()
                                 .replace("symbol", &item_to_source_string(item))
                                 .replace("number", &item_to_source_string(item))
                                 .replace("list", &item_to_source_string(item))
                         })
                         .collect::<Vec<_>>()
                         .join(" ")
                );
                
                Node::new(node_kind, code_snippet, children, metadata)
            } else {
                // Generic list
                metadata.insert("source_type".to_string(), "list".to_string());
                
                let code_snippet = format!(
                    "({})", 
                    items.iter()
                         .map(|item| item_to_source_string(item))
                         .collect::<Vec<_>>()
                         .join(" ")
                );
                
                Node::new(NodeKind::List, code_snippet, children, metadata)
            }
        }
    }
}

// Helper function to convert ExprAst back to a string representation for code snippets
fn item_to_source_string(item: &ExprAst) -> String {
    match item {
        ExprAst::Symbol(s, _) => s.clone(),
        ExprAst::Number(n, _) => n.to_string(),
        ExprAst::String(s, _) => format!("\"{}\"", s),
        ExprAst::List(items, _) => {
            format!("({})", items.iter().map(item_to_source_string).collect::<Vec<_>>().join(" "))
        }
    }
}

// Local future type that doesn't require Send
type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

// NodeCache manages caching of evaluated values by node ID
#[derive(Debug, Default)]
pub struct NodeCache {
    values: HashMap<NodeId, Result<Value, Error>>,
    last_update: HashMap<NodeId, String>, // ISO timestamp
    changed_nodes: std::collections::HashSet<NodeId>, // Tracks nodes changed in current run
    previously_seen: std::collections::HashSet<NodeId>, // Tracks all nodes seen before the current run
}

impl NodeCache {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            last_update: HashMap::new(),
            changed_nodes: std::collections::HashSet::new(),
            previously_seen: std::collections::HashSet::new(),
        }
    }
    
    pub fn get(&self, id: &NodeId) -> Option<&Result<Value, Error>> {
        self.values.get(id)
    }
    
    pub fn insert(&mut self, id: NodeId, value: Result<Value, Error>) {
        // A node is considered changed if:
        // 1. It didn't exist before OR
        // 2. Its value is different from the previous value
        let is_changed = match self.values.get(&id) {
            Some(old_value) => {
                // Compare the string representation of the values
                let old_str = format!("{:?}", old_value);
                let new_str = format!("{:?}", &value);
                old_str != new_str
            },
            None => true // New node
        };
        
        if is_changed {
            // Track this node as changed
            self.changed_nodes.insert(id);
            
            // Update the value and timestamp
            let now = chrono::Utc::now().to_rfc3339();
            self.last_update.insert(id, now);
        }
        
        // Always update the value, even if it hasn't changed
        self.values.insert(id, value);
    }
    
    // Check if a node was changed in the current cycle
    pub fn was_changed(&self, id: &NodeId) -> bool {
        self.changed_nodes.contains(id)
    }
    
    // Manually mark a node as changed (for propagating changes up the tree)
    pub fn mark_changed(&mut self, id: NodeId) {
        self.changed_nodes.insert(id);
    }
    
    // Before starting a new evaluation cycle, snapshot the current state
    pub fn prepare_for_evaluation(&mut self) {
        self.changed_nodes.clear();
        
        // Keep track of all nodes we've seen before
        for id in self.values.keys() {
            self.previously_seen.insert(*id);
        }
    }
    
    // Save cache to disk
    pub fn save_to_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Convert NodeIds to hex strings for JSON serialization
        let mut serializable_values = HashMap::new();
        
        for (id, value) in &self.values {
            let id_hex = hex::encode(id);
            
            // We need to convert Result<Value, Error> to a serializable format
            let value_str = match value {
                Ok(v) => serde_json::to_string(v)?,
                Err(e) => format!("Error: {}", e),
            };
            
            serializable_values.insert(id_hex, value_str);
        }
        
        let json = serde_json::to_string_pretty(&serializable_values)?;
        fs::write(path, json)?;
        Ok(())
    }
    
    // Load cache from disk
    pub fn load_from_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(());
        }
        
        
        let json = fs::read_to_string(path)?;
        let serializable_values: HashMap<String, String> = serde_json::from_str(&json)?;
        
        for (id_hex, value_str) in serializable_values {
            // Convert hex string back to NodeId
            let id_bytes = hex::decode(&id_hex)?;
            if id_bytes.len() != 32 {
                continue; // Skip invalid entries
            }
            let mut id = [0u8; 32];
            id.copy_from_slice(&id_bytes);
            
            // Try to parse as Value
            if let Ok(value) = serde_json::from_str::<Value>(&value_str) {
                self.values.insert(id, Ok(value));
                self.previously_seen.insert(id);
            } else {
                // It's an error string, store as error
                self.values.insert(id, Err(Error::EvalError(value_str.trim_start_matches("Error: ").to_string())));
                self.previously_seen.insert(id);
            }
            
            // Set timestamp to now
            self.last_update.insert(id, chrono::Utc::now().to_rfc3339());
        }
        
        Ok(())
    }
}

// Memoized evaluation of a Node tree
pub fn eval_node<'a>(node: &'a Rc<Node>, context: &'a mut IndexMap<String, Value>, cache: &'a mut NodeCache) 
    -> LocalBoxFuture<'a, Result<Value, Error>> {
    Box::pin(async move {
        // Get the node ID for easy reference
        let node_id = node.id;
        
        // Check if we already have a cached result for this node that isn't a symbol
        // Symbol nodes are re-evaluated if their underlying context value might have changed,
        // or if the symbol itself is part of a definition that changes.
        if !matches!(&node.kind, NodeKind::Symbol(_)) {
            if let Some(cached_value) = cache.get(&node_id) {
                 // If this node or any of its children were not marked as changed in this cycle,
                 // and it was seen before, we can potentially reuse the cache.
                 // However, for simplicity and correctness, especially with 'def',
                 // we will rely on the individual handlers to manage re-evaluation logic for now.
                 // The main check is that if a node's *dependencies* change, it *must* re-evaluate.
                 // The `cache.insert` at the end will determine if its own value changed.
                return cached_value.clone();
            }
        }
        
        // Evaluate this node based on its kind
        let result = match &node.kind {
            NodeKind::Symbol(name) => {
                // Symbol lookup
                // Check cache first, but symbols can change if context changes,
                // so a simple cache hit isn't always enough.
                // However, the 'changed_nodes' tracking should help.
                // If a 'def' changes a symbol, the 'def' node changes,
                // and any node *using* that symbol should ideally be re-evaluated.
                // This part is tricky and might need further refinement for optimal caching.
                // For now, direct lookup is fine, caching happens at the end.
                context.get(name)
                    .cloned()
                    .ok_or_else(|| Error::EvalError(format!("Undefined symbol: {}", name)))
            },
            NodeKind::Number(n) => {
                // Number literal
                Ok(Value::Number(*n))
            },
            NodeKind::String(s) => {
                // String literal
                Ok(Value::String(s.clone()))
            },
            NodeKind::Definition => {
                // Definition (def name value)
                // Children: 0: 'def' symbol, 1: name symbol, 2: value expression
                if node.children.len() != 3 {
                    return Err(Error::EvalError(format!(
                        "'def' expects 2 arguments (name, value), corresponding to 3 children (def, name, value). Got {} children.",
                        node.children.len()
                    )));
                }
                
                // Arg 1 (child 1) is the variable name symbol
                let var_name_node = &node.children[1];
                let var_name = if let NodeKind::Symbol(name) = &var_name_node.kind {
                    name.clone()
                } else {
                    return Err(Error::EvalError(
                        "'def' first argument must be a symbol representing the variable name".to_string(),
                    ));
                };

                // Arg 2 (child 2) is the value expression
                let value_expr_node = &node.children[2];
                let value = eval_node(value_expr_node, context, cache).await?;
                
                // Store in context
                context.insert(var_name.clone(), value.clone());
                
                // 'def' itself evaluates to the value assigned
                Ok(value)
            },
            NodeKind::Addition => {
                // Addition (+ a b c ...)
                // Children: 0: '+' symbol, 1...N: arguments
                if node.children.len() < 2 { // Needs at least operator and one arg for meaningful operation
                    return Err(Error::EvalError("'+' requires at least 1 argument".to_string()));
                }
                
                let mut sum = 0;
                // Evaluate argument children (starting from index 1)
                for i in 1..node.children.len() {
                    let arg_node = &node.children[i];
                    let val = eval_node(arg_node, context, cache).await?;
                    match val {
                        Value::Number(n) => sum += n,
                        _ => return Err(Error::EvalError(
                            "'+' requires all arguments to be numbers".to_string(),
                        )),
                    }
                }
                Ok(Value::Number(sum))
            },
            NodeKind::Multiplication => {
                // Multiplication (* a b c ...)
                // Children: 0: '*' symbol, 1...N: arguments
                if node.children.len() < 2 {
                    return Err(Error::EvalError("'*' requires at least 1 argument".to_string()));
                }
                
                let mut product = 1;
                // Evaluate argument children (starting from index 1)
                for i in 1..node.children.len() {
                    let arg_node = &node.children[i];
                    let val = eval_node(arg_node, context, cache).await?;
                    match val {
                        Value::Number(n) => product *= n,
                        _ => return Err(Error::EvalError(
                            "'*' requires all arguments to be numbers".to_string(),
                        )),
                    }
                }
                Ok(Value::Number(product))
            },
            NodeKind::HttpGet => {
                // HTTP GET (http.get url)
                // Children: 0: 'http.get' symbol, 1: url expression
                if node.children.len() != 2 {
                    return Err(Error::EvalError(
                        "'http.get' expects 1 argument (url), so 2 children in the node.".into(),
                    ));
                }
                
                // Evaluate the URL argument node (child 1)
                let url_expr_node = &node.children[1];
                match eval_node(url_expr_node, context, cache).await? {
                    Value::String(url) => {
                        // Perform the HTTP GET request
                        // This is an I/O operation, so it's inherently not "pure"
                        // Caching relies on the URL string itself. If URL changes, node hash changes.
                        // If content at URL changes but URL string doesn't, cache won't see it unless forced.
                        let body = reqwest::get(&url).await?.text().await?;
                        Ok(Value::String(body))
                    }
                    _ => Err(Error::EvalError(
                        "'http.get' expects its argument to evaluate to a string URL".into(),
                    )),
                }
            },
            NodeKind::JsonParse => {
                // JSON Parse (json.parse json_string)
                // Children: 0: 'json.parse' symbol, 1: string expression
                if node.children.len() != 2 {
                    return Err(Error::EvalError(
                        "'json.parse' expects 1 argument (a string to parse)".into(),
                    ));
                }
                
                // Evaluate the string argument node (child 1)
                let string_expr_node = &node.children[1];
                match eval_node(string_expr_node, context, cache).await? {
                    Value::String(s) => {
                        let json_data: JsonValue = serde_json::from_str(&s)?;
                        Ok(Value::Json(json_data))
                    }
                    _ => Err(Error::EvalError(
                        "'json.parse' expects its argument to evaluate to a string".into(),
                    )),
                }
            },
            NodeKind::JsonGet => {
                // JSON Get (get json_obj key_string)
                // Children: 0: 'get' symbol, 1: json_obj expression, 2: key_string expression
                if node.children.len() != 3 {
                    return Err(Error::EvalError(
                        "'get' expects 2 arguments (a JSON object, a string key)".into(),
                    ));
                }
                
                // Evaluate the JSON object argument (child 1)
                let json_obj_expr_node = &node.children[1];
                let json_val = eval_node(json_obj_expr_node, context, cache).await?;
                
                // Evaluate the key string argument (child 2)
                let key_string_expr_node = &node.children[2];
                let key_val = eval_node(key_string_expr_node, context, cache).await?;
                
                match (json_val, key_val) {
                    (Value::Json(json_data), Value::String(key)) => {
                        match json_data.get(&key) {
                            Some(v) => convert_json_value(v.clone()), // convert_json_value handles errors for unsupported types
                            None => Err(Error::EvalError(format!(
                                "Key '{}' not found in JSON object",
                                key
                            ))),
                        }
                    }
                    (Value::Json(_), other_key_type) => Err(Error::EvalError(format!(
                        "'get' expects the second argument (key) to be a string, got {:?}",
                        other_key_type
                    ))),
                    (other_json_type, _) => Err(Error::EvalError(format!(
                        "'get' expects the first argument to be a JSON object, got {:?}",
                        other_json_type
                    ))),
                }
            },
            NodeKind::StringUpper => {
                // String to uppercase (str.upper string_expr)
                // Children: 0: 'str.upper' symbol, 1: string expression
                if node.children.len() != 2 {
                    return Err(Error::EvalError(
                        "'str.upper' expects 1 argument (a string)".into(),
                    ));
                }
                
                // Evaluate the string argument (child 1)
                let string_expr_node = &node.children[1];
                match eval_node(string_expr_node, context, cache).await? {
                    Value::String(s) => Ok(Value::String(s.to_uppercase())),
                    other_type => Err(Error::EvalError(format!(
                        "'str.upper' expects its argument to evaluate to a string, got {:?}",
                        other_type
                    ))),
                }
            },
            NodeKind::List => {
                // Generic list or unknown function call
                if node.children.is_empty() {
                    // This case should ideally be caught by the parser or ast_to_node_tree
                    // Or result in a NodeKind that's not generic List if it's ()
                    return Err(Error::EvalError("Cannot evaluate an empty list node directly. If it's an empty S-expression '()', its NodeKind should reflect that.".to_string()));
                }
                
                // The first child of a List node (if not a special form handled above)
                // would be the function to call.
                let func_expr_node = &node.children[0];
                
                // What is it? If it's a symbol, it's an attempt to call a function by that name.
                if let NodeKind::Symbol(func_name) = &func_expr_node.kind {
                    // Here, we would look up `func_name` in the context.
                    // If it's a user-defined function (not yet supported by this interpreter)
                    // or a built-in that wasn't converted to a specific NodeKind (e.g. if '+' was a generic List),
                    // we'd handle it.
                    // For now, unknown symbols as functions are errors.
                    Err(Error::EvalError(format!(
                        "Attempted to call '{}' as a function, but it's either undefined or not a known built-in operation recognized by its specific NodeKind. Code: '{}'",
                        func_name, node.code_snippet
                    )))
                } else {
                    // If the head of the list is not a symbol, it's an error (e.g. ((+ 1 2) 3))
                    Err(Error::EvalError(format!(
                        "The first element of a list to be evaluated as a function call must be a symbol. Got: {:?}. Code: '{}'",
                        func_expr_node.kind, node.code_snippet
                    )))
                }
            }
        };
        
        // Cache the result. `cache.insert` will also handle marking the node as changed
        // if its new value is different from a previously cached one, or if it's new.
        cache.insert(node_id, result.clone());
        
        result
    })
}


