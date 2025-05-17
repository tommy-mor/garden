use std::{collections::{HashMap, HashSet}, fs, path::Path, sync::mpsc, rc::Rc, hash::{Hash, Hasher}, time::{Instant, Duration}};
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use indexmap::IndexMap;
use reqwest;
use futures::future::{BoxFuture, Future};
use std::pin::Pin;
use notify::{Watcher, RecursiveMode, recommended_watcher};
use chrono::{self, DateTime, Utc};
use blake3;
use hex;

// === Import chrono with serde features ===
#[cfg(feature = "serde")]
use chrono::{serde::ts_seconds, Duration};

// Add pest parser module
mod parser;

// === TYPES ===

#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpan {
    pub line: usize,
    pub original_text: String, // Store the original source text
}

type NodeId = [u8; 32]; // 32 bytes for BLAKE3 hash

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Symbol(String),
    Number(i64),
    String(String),
    List,
    // More specific operations
    Definition,
    LetExpr,        // Changed from Let: (let name value body) - expression form
    LetStatement,   // New: (let name value) - statement form, modifies current env
    Addition,
    Multiplication,
    HttpGet,
    JsonParse,
    JsonGet,
    StringUpper,
}

// Immutable computation tree node without cached_value (moved to EvaluationCache)
#[derive(Debug, Clone)]
pub struct Node {
    id: NodeId,                       // Content-based hash for identity
    kind: NodeKind,                   // The kind of operation this node represents
    code_snippet: String,             // Original source code
    children: Vec<Rc<Node>>,          // Child nodes - immutable references
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
            NodeKind::LetExpr => {
                hasher.update(b"LetExpr");
            }
            NodeKind::LetStatement => {
                hasher.update(b"LetStatement");
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
    
    // Get node kind
    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }
    
    // Get children
    pub fn children(&self) -> &[Rc<Node>] {
        &self.children
    }
    
    // Get code snippet
    pub fn code_snippet(&self) -> &str {
        &self.code_snippet
    }
    
    // Get metadata
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
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

// Environment for lexical scoping
#[derive(Debug, Clone)]
pub struct Env<'parent> {
    bindings: HashMap<String, NodeId>,
    parent: Option<&'parent Env<'parent>>,
}

impl<'parent> Env<'parent> {
    // Create a new empty environment
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            parent: None,
        }
    }
    
    // Create a new environment with a parent for lexical scoping
    pub fn with_parent(parent: &'parent Env<'parent>) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    // Resolve a symbol to its defining NodeId
    pub fn resolve(&self, name: &str) -> Option<NodeId> {
        if let Some(node_id) = self.bindings.get(name) {
            Some(*node_id)
        } else if let Some(parent) = self.parent {
            parent.resolve(name)
        } else {
            None
        }
    }
    
    // Add or update a binding
    pub fn bind(&mut self, name: &str, node_id: NodeId) {
        self.bindings.insert(name.to_string(), node_id);
    }
    
    // Create a new environment extending this one with new bindings
    pub fn extend(&self, new_bindings: HashMap<String, NodeId>) -> Env {
        let mut env = Env::with_parent(self);
        for (name, node_id) in new_bindings {
            env.bind(&name, node_id);
        }
        env
    }
}

// Cached evaluation result with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedValue {
    result: Result<Value, Error>,
    #[serde(with = "chrono::serde::ts_seconds")]
    timestamp: DateTime<Utc>,
}

// The unified evaluation cache
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct EvaluationCache {
    #[serde(serialize_with = "node_id_map_serde::serialize_cached_values_map", 
            deserialize_with = "node_id_map_serde::deserialize_cached_values_map")]
    cache: HashMap<NodeId, CachedValue>,
    
    #[serde(skip)]
    changed_nodes: HashSet<NodeId>,
    
    #[serde(skip)]
    all_nodes: HashMap<NodeId, Rc<Node>>,
}

// Serde helper module for NodeId maps
mod node_id_map_serde {
    use serde::{
        de::Error as SerdeError, ser::SerializeMap, Deserializer, Serializer,
        Serialize, Deserialize
    };
    use std::collections::HashMap;
    use super::{NodeId, Value, Error as EvalError, CachedValue};
    use hex;
    
    // For HashMap<NodeId, CachedValue>
    pub fn serialize_cached_values_map<S>(
        map: &HashMap<NodeId, CachedValue>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut smap = serializer.serialize_map(Some(map.len()))?;
        for (k, v) in map {
            let k_hex = hex::encode(k);
            smap.serialize_entry(&k_hex, v)?;
        }
        smap.end()
    }

    pub fn deserialize_cached_values_map<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<NodeId, CachedValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map = HashMap::<String, CachedValue>::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for (k_hex, v) in string_map {
            let mut node_id = [0u8; 32];
            hex::decode_to_slice(&k_hex, &mut node_id).map_err(SerdeError::custom)?;
            map.insert(node_id, v);
        }
        Ok(map)
    }
}

impl EvaluationCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            changed_nodes: HashSet::new(),
            all_nodes: HashMap::new(),
        }
    }
    
    // Get cached value for a node
    pub fn get(&self, id: &NodeId) -> Option<&Result<Value, Error>> {
        self.cache.get(id).map(|cached| &cached.result)
    }
    
    // Insert a new evaluation result
    pub fn insert(&mut self, id: NodeId, result: Result<Value, Error>) {
        let is_changed = match self.cache.get(&id) {
            Some(old_cached) => {
                let old_str = format!("{:?}", old_cached.result);
                let new_str = format!("{:?}", &result);
                old_str != new_str
            },
            None => true // New node
        };
        
        if is_changed {
            self.changed_nodes.insert(id);
        }
        
        self.cache.insert(id, CachedValue {
            result,
            timestamp: chrono::Utc::now(),
        });
    }
    
    // Check if a node's value changed in this evaluation cycle
    pub fn was_changed(&self, id: &NodeId) -> bool {
        self.changed_nodes.contains(id)
    }
    
    // Store a node in the all_nodes map
    pub fn store_node(&mut self, node: Rc<Node>) {
        self.all_nodes.insert(*node.id(), node);
    }
    
    // Get a node by ID
    pub fn get_node(&self, id: &NodeId) -> Option<&Rc<Node>> {
        self.all_nodes.get(id)
    }
    
    // Clear the changed_nodes set to prepare for a new evaluation cycle
    pub fn prepare_for_evaluation(&mut self) {
        self.changed_nodes.clear();
    }
    
    // Save cache to file
    pub fn save_to_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self)?;
        fs::write(path, json)?;
        Ok(())
    }
    
    // Load cache from file
    pub fn load_from_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if !path.exists() {
            *self = EvaluationCache::default();
            return Ok(());
        }
        
        let json_str = fs::read_to_string(path)?;
        if json_str.trim().is_empty() {
            *self = EvaluationCache::default();
            return Ok(());
        }
        
        match serde_json::from_str::<EvaluationCache>(&json_str) {
            Ok(loaded_cache) => {
                self.cache = loaded_cache.cache;
                // Ensure transient fields are correctly initialized after load
                self.changed_nodes = HashSet::new();
            },
            Err(e) => {
                eprintln!("Failed to load evaluation cache, reinitializing: {}", e);
                *self = EvaluationCache::default();
            }
        }
        Ok(())
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

// The evaluator/runtime that manages evaluation of the node tree
#[derive(Debug)]
pub struct Evaluator {
    cache: EvaluationCache,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            cache: EvaluationCache::new(),
        }
    }
    
    // Load cache from file
    pub fn load_cache(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.cache.load_from_file(path)
    }
    
    // Save cache to file
    pub fn save_cache(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.cache.save_to_file(path)
    }
    
    // Store a node in the cache
    pub fn store_node(&mut self, node: Rc<Node>) {
        self.cache.store_node(node.clone());
        
        // Also store all children recursively
        for child in node.children() {
            self.store_node(child.clone());
        }
    }
    
    // Prepare for a new evaluation cycle
    pub fn prepare_for_evaluation(&mut self) {
        self.cache.prepare_for_evaluation();
    }
    
    // Get a list of all nodes that changed in the last evaluation cycle
    pub fn get_changed_nodes(&self) -> Vec<Rc<Node>> {
        self.cache.changed_nodes.iter()
            .filter_map(|id| self.cache.get_node(id).cloned())
            .collect()
    }
    
    // Get cached result to avoid borrow issues
    fn get_cached_result(&self, id: &NodeId) -> Option<Result<Value, Error>> {
        self.cache.get(id).cloned()
    }
    
    // Get node from cache
    fn get_node(&self, id: &NodeId) -> Option<Rc<Node>> {
        self.cache.get_node(id).cloned()
    }
    
    // Evaluate a node asynchronously
    pub fn eval_node<'a>(&'a mut self, node: &'a Rc<Node>, env: &'a Env<'a>) -> LocalBoxFuture<'a, Result<Value, Error>> {
        Box::pin(async move {
            // Get the node ID for easy reference
            let node_id = *node.id();
            
            // Check if we have a cached value - avoid borrow issues by getting a clone before the mutable borrow
            if let Some(cached_result) = self.get_cached_result(&node_id) {
                return cached_result;
            }
            
            // For symbol nodes, we need to resolve and evaluate the defining node
            if let NodeKind::Symbol(name) = node.kind() {
                let result = match env.resolve(name) {
                    Some(defining_node_id) => {
                        match self.get_node(&defining_node_id) {
                            Some(defining_node) => self.eval_node(&defining_node, env).await,
                            None => Err(Error::EvalError(format!("Internal error: Symbol {} resolved to unknown node", name)))
                        }
                    },
                    None => Err(Error::EvalError(format!("Undefined symbol: {}", name)))
                };
                self.cache.insert(node_id, result.clone());
                return result;
            }
            
            // For other node types, proceed with normal evaluation
            let result = match node.kind() {
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
                    if node.children().len() != 3 {
                        return Err(Error::EvalError(format!(
                            "'def' expects 2 arguments (name, value), got {} arguments",
                            node.children().len() - 1
                        )));
                    }
                    
                    // Arg 1 (child 1) is the variable name symbol
                    let var_name_node = &node.children()[1];
                    let var_name = if let NodeKind::Symbol(name) = var_name_node.kind() {
                        name.clone()
                    } else {
                        return Err(Error::EvalError(
                            "'def' first argument must be a symbol representing the variable name".to_string(),
                        ));
                    };

                    // Arg 2 (child 2) is the value expression
                    let value_expr_node = &node.children()[2];
                    let value = self.eval_node(value_expr_node, env).await?;
                    
                    // Update the environment with this binding
                    let mut env = env.clone();
                    env.bind(&var_name, *value_expr_node.id());
                    
                    // 'def' itself evaluates to the value assigned
                    Ok(value)
                },
                NodeKind::LetExpr => {
                    // Let binding (let name value body)
                    // Children: 0: 'let' symbol, 1: name symbol, 2: value expression, 3: body expression
                    if node.children().len() != 4 {
                        return Err(Error::EvalError(format!(
                            "'let' expects 3 arguments (name, value, body), got {} arguments",
                            node.children().len() - 1
                        )));
                    }
                    
                    // Arg 1 (child 1) is the variable name symbol
                    let var_name_node = &node.children()[1];
                    let var_name = if let NodeKind::Symbol(name) = var_name_node.kind() {
                        name.clone()
                    } else {
                        return Err(Error::EvalError(
                            "'let' first argument must be a symbol representing the variable name".to_string(),
                        ));
                    };

                    // Arg 2 (child 2) is the value expression
                    let value_expr_node = &node.children()[2];
                    
                    // Evaluate the value expression in the current environment
                    let value = self.eval_node(value_expr_node, env).await?;
                    
                    // Create a new environment extending the current one with the new binding
                    let mut new_bindings = HashMap::new();
                    new_bindings.insert(var_name, *value_expr_node.id());
                    let new_env = env.extend(new_bindings);
                    
                    // Evaluate the body expression in the new environment
                    let body_expr_node = &node.children()[3];
                    let body_result = self.eval_node(body_expr_node, &new_env).await?;
                    
                    Ok(body_result)
                },
                NodeKind::LetStatement => {
                    // Let statement (let name value)
                    // Children: 0: 'let' symbol, 1: name symbol, 2: value expression
                    if node.children().len() != 3 {
                        return Err(Error::EvalError(format!(
                            "'let' statement expects 2 arguments (name, value), got {} arguments",
                            node.children().len() - 1
                        )));
                    }
                    
                    // Arg 1 (child 1) is the variable name symbol
                    let var_name_node = &node.children()[1];
                    if let NodeKind::Symbol(_) = var_name_node.kind() {
                        // We don't actually bind anything here - that's done by evaluate_sequence
                        // We just validate the structure and evaluate the value
                    } else {
                        return Err(Error::EvalError(
                            "'let' statement first argument must be a symbol representing the variable name".to_string(),
                        ));
                    };

                    // Arg 2 (child 2) is the value expression
                    let value_expr_node = &node.children()[2];
                    let value = self.eval_node(value_expr_node, env).await?;
                    
                    // LetStatement evaluates to the value assigned
                    Ok(value)
                },
                NodeKind::Addition => {
                    // Addition (+ a b c ...)
                    if node.children().len() < 2 {
                        return Err(Error::EvalError("'+' requires at least 1 argument".to_string()));
                    }
                    
                    let mut sum = 0;
                    // Evaluate argument children (starting from index 1)
                    for i in 1..node.children().len() {
                        let arg_node = &node.children()[i];
                        let val = self.eval_node(arg_node, env).await?;
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
                    if node.children().len() < 2 {
                        return Err(Error::EvalError("'*' requires at least 1 argument".to_string()));
                    }
                    
                    let mut product = 1;
                    // Evaluate argument children (starting from index 1)
                    for i in 1..node.children().len() {
                        let arg_node = &node.children()[i];
                        let val = self.eval_node(arg_node, env).await?;
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
                    if node.children().len() != 2 {
                        return Err(Error::EvalError(
                            "'http.get' expects 1 argument (url), so 2 children in the node.".into(),
                        ));
                    }
                    
                    // Evaluate the URL argument node (child 1)
                    let url_expr_node = &node.children()[1];
                    match self.eval_node(url_expr_node, env).await? {
                        Value::String(url) => {
                            // Perform the HTTP GET request
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
                    if node.children().len() != 2 {
                        return Err(Error::EvalError(
                            "'json.parse' expects 1 argument (a string to parse)".into(),
                        ));
                    }
                    
                    // Evaluate the string argument node (child 1)
                    let string_expr_node = &node.children()[1];
                    match self.eval_node(string_expr_node, env).await? {
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
                    if node.children().len() != 3 {
                        return Err(Error::EvalError(
                            "'get' expects 2 arguments (a JSON object, a string key)".into(),
                        ));
                    }
                    
                    // Evaluate the JSON object argument (child 1)
                    let json_obj_expr_node = &node.children()[1];
                    let json_val = self.eval_node(json_obj_expr_node, env).await?;
                    
                    // Evaluate the key string argument (child 2)
                    let key_string_expr_node = &node.children()[2];
                    let key_val = self.eval_node(key_string_expr_node, env).await?;
                    
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
                    if node.children().len() != 2 {
                        return Err(Error::EvalError(
                            "'str.upper' expects 1 argument (a string)".into(),
                        ));
                    }
                    
                    // Evaluate the string argument (child 1)
                    let string_expr_node = &node.children()[1];
                    match self.eval_node(string_expr_node, env).await? {
                        Value::String(s) => Ok(Value::String(s.to_uppercase())),
                        other_type => Err(Error::EvalError(format!(
                            "'str.upper' expects its argument to evaluate to a string, got {:?}",
                            other_type
                        ))),
                    }
                },
                NodeKind::List => {
                    // Generic list or unknown function call
                    if node.children().is_empty() {
                        return Err(Error::EvalError("Cannot evaluate an empty list".to_string()));
                    }
                    
                    // The first child of a List node (if not a special form handled above)
                    // would be the function to call.
                    let func_expr_node = &node.children()[0];
                    
                    // What is it? If it's a symbol, it's an attempt to call a function by that name.
                    if let NodeKind::Symbol(func_name) = func_expr_node.kind() {
                        Err(Error::EvalError(format!(
                            "Attempted to call '{}' as a function, but it's either undefined or not a known built-in operation",
                            func_name
                        )))
                    } else {
                        Err(Error::EvalError(
                            "The first element of a list to be evaluated as a function call must be a symbol".to_string()
                        ))
                    }
                },
                // Unexpected node types
                NodeKind::Symbol(_) => {
                    // Should be handled above already
                    Err(Error::EvalError("Reached unreachable code: Symbol handling in match".to_string()))
                }
            };
            
            // Cache the result
            self.cache.insert(node_id, result.clone());
            
            result
        })
    }

    // Evaluate a sequence of nodes in order, updating the environment for definitions and let statements
    pub async fn evaluate_sequence<'a>(
        &'a mut self,
        nodes: &'a [Rc<Node>],
        env: &'a mut Env<'a>,
    ) -> Result<Option<Value>, Error> {
        let mut last_value = None;

        for node in nodes {
            let node_id = *node.id();
            let result = self.eval_node(node, env).await;
            
            // For Definition and LetStatement nodes, also update the environment
            match node.kind() {
                NodeKind::Definition | NodeKind::LetStatement => {
                    if node.children().len() >= 3 {
                        if let NodeKind::Symbol(name) = node.children()[1].kind() {
                            if result.is_ok() {
                                // Bind the name to the value expression NodeId for future lookups
                                env.bind(name, *node.children()[2].id());
                            }
                        }
                    }
                },
                _ => {} // Other node types don't modify the environment
            }
            
            // Remember the result of this node
            if let Ok(value) = &result {
                last_value = Some(value.clone());
            }
            
            // If there was an error and it hasn't been inserted into the cache yet, insert it
            if let Err(err) = &result {
                self.cache.insert(node_id, Err(err.clone()));
                return Err(err.clone());
            }
        }
        
        Ok(last_value)
    }
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

// New struct for display
#[derive(Debug)]
struct DisplayInfo {
    line: usize,
    code_snippet: String,
    id_hex_short: String, // Short version of NodeId hex
    value_str: String,    // String representation of the Value or Error
}

// Main function
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: garden <file.expr>");
        return Ok(());
    }
    
    let file_path = Path::new(&args[1]);
    let cache_path = file_path.with_extension("expr.cache");
    
    // Initialize the evaluator
    let mut evaluator = Evaluator::new();
    
    // Try to load previous cache
    if let Err(e) = evaluator.load_cache(&cache_path) {
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
    
    // Initial run
    if let Err(e) = run_once(file_path, &mut evaluator).await {
        eprintln!("Error: {}", e);
    }
    
    // Save cache
    if let Err(e) = evaluator.save_cache(&cache_path) {
        eprintln!("Warning: Could not save cache: {}", e);
    }
    
    // Event loop
    for res in rx {
        match res {
            Ok(_) => {
                if let Err(e) = run_once(file_path, &mut evaluator).await {
                    eprintln!("Error: {}", e);
                } else {
                    // Save cache after successful run
                    if let Err(e) = evaluator.save_cache(&cache_path) {
                        eprintln!("Warning: Could not save cache: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }
    
    Ok(())
}

async fn run_once(path: &Path, evaluator: &mut Evaluator) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nRevaluating expressions in {}...", path.display());
    
    evaluator.prepare_for_evaluation();
    
    let src = fs::read_to_string(path)?;
    
    // Parse the source file into a vector of root nodes
    // Use the parse function directly as it now returns nodes
    let root_nodes = parser::parse(&src)?;
    
    // Create a top-level environment
    let mut env = Env::new();
    
    // Store all nodes in the evaluator
    for node in &root_nodes {
        evaluator.store_node(node.clone());
    }
    
    // Evaluate the sequence of root nodes, updating env for definitions and let statements
    if let Err(e) = evaluator.evaluate_sequence(&root_nodes, &mut env).await {
        eprintln!("Evaluation error: {}", e);
    }
    
    // Get all changed nodes for display
    let changed_nodes = evaluator.get_changed_nodes();
    
    // Convert to DisplayInfo
    let mut display_items: Vec<DisplayInfo> = Vec::new();
    for node in &changed_nodes {
        let line_str = node.metadata().get("line")
            .expect("Node metadata should contain 'line' information");
        let line = line_str.parse::<usize>()
            .expect("Line metadata should be a parsable usize");
        
        let id_hex_short = hex::encode(&node.id()[0..4]); // First 4 bytes for display
        
        let value_representation = match evaluator.get_cached_result(node.id()) {
            Some(Ok(value)) => format!("{:?}", value),
            Some(Err(error)) => format!("Error: {}", error),
            None => "Value not cached (Error: should not happen for a changed node)".to_string(),
        };
        
        display_items.push(DisplayInfo {
            line,
            code_snippet: node.code_snippet().to_string(),
            id_hex_short,
            value_str: value_representation,
        });
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


