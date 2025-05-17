use std::{collections::HashMap, fs, path::Path, iter::Peekable, str::Chars, sync::mpsc, time::Duration};
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use indexmap::IndexMap;
use reqwest;
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

#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpan {
    pub line: usize,
    pub original_text: String, // Store the original source text
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprAst {
    Symbol(String, SourceSpan),
    Number(i64, SourceSpan),
    List(Vec<ExprAst>, SourceSpan),
    String(String, SourceSpan),
}

type NodeId = [u8; 32]; // 32 bytes for BLAKE3 hash

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        
        let now_str = chrono::Utc::now().to_rfc3339();
        for key in self.values.keys() {
            self.last_update.insert(key.clone(), now_str.clone());
        }
        Ok(())
    }
}

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

// Convert from ExprAst to Node tree
fn ast_to_node_tree(ast: &ExprAst) -> Rc<Node> {
    let mut metadata = HashMap::new();
    
    // Extract span info including line and original text
    let span = match ast {
        ExprAst::Symbol(_, s) => s,
        ExprAst::Number(_, s) => s,
        ExprAst::List(_, s) => s,
        ExprAst::String(_, s) => s,
    };
    
    // Store line information in metadata
    metadata.insert("line".to_string(), span.line.to_string());

    match ast {
        ExprAst::Symbol(s, _) => {
            metadata.insert("source_type".to_string(), "symbol".to_string());
            Node::new(
                NodeKind::Symbol(s.clone()),
                s.clone(), // Already have symbol name
                Vec::new(),
                metadata
            )
        },
        ExprAst::Number(n, _) => {
            metadata.insert("source_type".to_string(), "number".to_string());
            Node::new(
                NodeKind::Number(*n),
                n.to_string(), // Already have number as string
                Vec::new(),
                metadata
            )
        },
        ExprAst::String(s, _) => {
            metadata.insert("source_type".to_string(), "string".to_string());
            // Use span's original text which includes quotes
            Node::new(
                NodeKind::String(s.clone()),
                span.original_text.clone(),
                Vec::new(),
                metadata
            )
        },
        ExprAst::List(items, _) => {
            if items.is_empty() {
                metadata.insert("source_type".to_string(), "empty_list".to_string());
                return Node::new(
                    NodeKind::List,
                    span.original_text.clone(), // Use original text
                    Vec::new(),
                    metadata
                );
            }
            
            // Create child nodes for ALL items including the operator
            let children: Vec<Rc<Node>> = items.iter().map(ast_to_node_tree).collect();
            
            // Determine the operation type from the first item if it's a symbol
            if let ExprAst::Symbol(op, _) = &items[0] {
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
                
                // Use original source text directly
                Node::new(node_kind, span.original_text.clone(), children, metadata)
            } else {
                // Generic list
                metadata.insert("source_type".to_string(), "list".to_string());
                Node::new(NodeKind::List, span.original_text.clone(), children, metadata)
            }
        }
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

type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

// NodeCache manages caching of evaluated values by node ID
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeCache {
    #[serde(serialize_with = "node_id_map_serde::serialize_result_values_map", 
            deserialize_with = "node_id_map_serde::deserialize_result_values_map")]
    values: HashMap<NodeId, Result<Value, Error>>,
    
    #[serde(serialize_with = "node_id_map_serde::serialize_node_id_string_map",
            deserialize_with = "node_id_map_serde::deserialize_node_id_string_map")]
    last_update: HashMap<NodeId, String>, // ISO timestamp
    
    #[serde(serialize_with = "node_id_map_serde::serialize_string_node_id_set_map",
            deserialize_with = "node_id_map_serde::deserialize_string_node_id_set_map")]
    dependencies: HashMap<String, std::collections::HashSet<NodeId>>, // Maps symbol names to nodes that depend on them

    #[serde(skip)] // Transient fields, not serialized
    changed_nodes: std::collections::HashSet<NodeId>,
    #[serde(skip)]
    previously_seen: std::collections::HashSet<NodeId>,
}

// Serde helper module for NodeId maps
mod node_id_map_serde {
    use serde::{
        de::Error as SerdeError, ser::SerializeMap, Deserializer, Serializer,
        Serialize, Deserialize
    };
    use std::collections::HashMap;
    use crate::{NodeId, Value, Error as EvalError};
    use hex;

    // For HashMap<NodeId, Result<Value, EvalError>> (values field)
    pub fn serialize_result_values_map<S>(
        map: &HashMap<NodeId, Result<Value, EvalError>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut smap = serializer.serialize_map(Some(map.len()))?;
        for (k, v_res) in map {
            let k_hex = hex::encode(k);
            let v_str = match v_res {
                Ok(val) => serde_json::to_string(val).map_err(serde::ser::Error::custom)?,
                Err(err) => format!("Error: {}", err),
            };
            smap.serialize_entry(&k_hex, &v_str)?;
        }
        smap.end()
    }

    pub fn deserialize_result_values_map<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<NodeId, Result<Value, EvalError>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map = HashMap::<String, String>::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for (k_hex, v_str) in string_map {
            let mut node_id = [0u8; 32];
            hex::decode_to_slice(&k_hex, &mut node_id).map_err(SerdeError::custom)?;
            if let Ok(val) = serde_json::from_str::<Value>(&v_str) {
                map.insert(node_id, Ok(val));
            } else {
                // Assuming it's an error string if not a Value
                map.insert(node_id, Err(EvalError::EvalError(v_str.trim_start_matches("Error: ").to_string())));
            }
        }
        Ok(map)
    }

    // For HashMap<NodeId, String> (last_update field)
    pub fn serialize_node_id_string_map<S>(
        map: &HashMap<NodeId, String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut smap = serializer.serialize_map(Some(map.len()))?;
        for (k, v) in map {
            smap.serialize_entry(&hex::encode(k), v)?;
        }
        smap.end()
    }

    pub fn deserialize_node_id_string_map<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<NodeId, String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map = HashMap::<String, String>::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for (k_hex, v) in string_map {
            let mut node_id = [0u8; 32];
            hex::decode_to_slice(&k_hex, &mut node_id).map_err(SerdeError::custom)?;
            map.insert(node_id, v);
        }
        Ok(map)
    }

    // For HashMap<String, HashSet<NodeId>> (dependencies field)
    pub fn serialize_string_node_id_set_map<S>(
        map: &HashMap<String, std::collections::HashSet<NodeId>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut smap = serializer.serialize_map(Some(map.len()))?;
        for (k_str, v_set) in map {
            let v_hex_vec: Vec<String> = v_set.iter().map(hex::encode).collect();
            smap.serialize_entry(k_str, &v_hex_vec)?;
        }
        smap.end()
    }

    pub fn deserialize_string_node_id_set_map<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, std::collections::HashSet<NodeId>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map = HashMap::<String, Vec<String>>::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for (k_str, v_hex_vec) in string_map {
            let mut node_id_set = std::collections::HashSet::new();
            for id_hex in v_hex_vec {
                let mut node_id = [0u8; 32];
                hex::decode_to_slice(&id_hex, &mut node_id).map_err(SerdeError::custom)?;
                node_id_set.insert(node_id);
            }
            map.insert(k_str, node_id_set);
        }
        Ok(map)
    }
}

impl Default for NodeCache {
    fn default() -> Self {
        NodeCache {
            values: HashMap::new(),
            last_update: HashMap::new(),
            changed_nodes: std::collections::HashSet::new(), // Initialized empty
            previously_seen: std::collections::HashSet::new(), // Initialized empty
            dependencies: HashMap::new(),
        }
    }
}

impl NodeCache {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn get(&self, id: &NodeId) -> Option<&Result<Value, Error>> {
        self.values.get(id)
    }
    
    pub fn insert(&mut self, id: NodeId, value: Result<Value, Error>) {
        let is_changed = match self.values.get(&id) {
            Some(old_value) => {
                let old_str = format!("{:?}", old_value);
                let new_str = format!("{:?}", &value);
                old_str != new_str
            },
            None => true // New node
        };
        
        if is_changed {
            self.changed_nodes.insert(id);
            let now = chrono::Utc::now().to_rfc3339();
            self.last_update.insert(id, now);
        }
        self.values.insert(id, value);
    }
    
    pub fn add_dependency(&mut self, symbol_name: &str, node_id: NodeId) {
        self.dependencies
            .entry(symbol_name.to_string())
            .or_insert_with(std::collections::HashSet::new)
            .insert(node_id);
    }
    
    pub fn mark_dependents_changed(&mut self, symbol_name: &str) {
        if let Some(dependent_nodes) = self.dependencies.get(symbol_name) {
            for &node_id in dependent_nodes.iter() {
                self.changed_nodes.insert(node_id);
            }
        }
    }
    
    pub fn was_changed(&self, id: &NodeId) -> bool {
        self.changed_nodes.contains(id)
    }
    
    pub fn mark_changed(&mut self, id: NodeId) {
        self.changed_nodes.insert(id);
    }
    
    pub fn prepare_for_evaluation(&mut self) {
        self.changed_nodes.clear();
        self.previously_seen.clear();
        for id in self.values.keys() {
            self.previously_seen.insert(*id);
        }
    }
    
    pub fn save_to_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self)?;
        fs::write(path, json)?;
        Ok(())
    }
    
    pub fn load_from_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if !path.exists() {
            *self = NodeCache::default();
            return Ok(());
        }
        let json_str = fs::read_to_string(path)?;
        if json_str.trim().is_empty() {
            *self = NodeCache::default();
            return Ok(());
        }
        
        match serde_json::from_str::<NodeCache>(&json_str) {
            Ok(loaded_cache) => {
                *self = loaded_cache;
                // Ensure transient fields are correctly initialized after load
                self.changed_nodes = std::collections::HashSet::new();
                self.previously_seen = std::collections::HashSet::new();
                for id in self.values.keys() {
                    self.previously_seen.insert(*id);
                }
            },
            Err(e) => {
                eprintln!("Failed to load node cache, reinitializing: {}", e);
                *self = NodeCache::default();
            }
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
        
        // Check if the node is already marked as changed, or if it's a symbol (which might have new values)
        // For non-symbol nodes, check if they're marked as changed
        if !matches!(&node.kind, NodeKind::Symbol(_)) && !cache.was_changed(&node_id) {
            if let Some(cached_value) = cache.get(&node_id) {
                return cached_value.clone();
            }
        }
        
        // Evaluate this node based on its kind
        let result = match &node.kind {
            NodeKind::Symbol(name) => {
                // Record dependency BEFORE lookup to avoid borrowing issues
                cache.add_dependency(name, node_id);
                
                // Look up the symbol in the context
                let result = context.get(name)
                    .cloned()
                    .ok_or_else(|| Error::EvalError(format!("Undefined symbol: {}", name)))?;
                
                Ok(result)
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
                
                // Get old value of the variable if it exists
                let old_value = context.get(&var_name).cloned();
                
                // Store in context
                context.insert(var_name.clone(), value.clone());
                
                // If the value changed, mark all nodes that depend on this symbol as changed
                if old_value.is_none() || old_value.as_ref() != Some(&value) {
                    cache.mark_dependents_changed(&var_name);
                }
                
                // 'def' itself evaluates to the value assigned
                Ok(value)
            },
            NodeKind::Addition => {
                // Addition (+ a b c ...)
                if node.children.len() < 2 {
                    return Err(Error::EvalError("'+' requires at least 1 argument".to_string()));
                }
                
                let mut sum = 0;
                // Evaluate argument children (starting from index 1)
                for i in 1..node.children.len() {
                    let arg_node = &node.children[i];
                    
                    // Record dependency if it's a symbol
                    if let NodeKind::Symbol(name) = &arg_node.kind {
                        cache.add_dependency(name, node_id);
                    }
                    
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
                if node.children.len() < 2 {
                    return Err(Error::EvalError("'*' requires at least 1 argument".to_string()));
                }
                
                let mut product = 1;
                // Evaluate argument children (starting from index 1)
                for i in 1..node.children.len() {
                    let arg_node = &node.children[i];
                    
                    // Record dependency if it's a symbol
                    if let NodeKind::Symbol(name) = &arg_node.kind {
                        cache.add_dependency(name, node_id);
                    }
                    
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


