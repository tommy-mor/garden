Okay, let's scope out a minimal viable product (MVP) – `garden-py` v0.0.0 – for the "PyObjects All The Way Down" vision. The goal is to demonstrate the core workflow loop: **write code -> see value -> change code -> see value update**, using Python syntax and objects, with caching and dependency awareness, even if extremely limited initially.

**Core Goal of v0.0.0:**

Execute a *tiny* subset of Python, tracking dependencies between simple assignments and binary operations, caching the resulting `PyObject`s *in memory*, and displaying these values next to the source code lines in a basic TUI that updates on file change.

**Key Components & Technologies:**

1.  **Language:** Rust (for performance, safety, `pyo3`).
2.  **Python Interop:** `pyo3` (essential for `PyObject` representation and C-API calls).
3.  **Parsing:** `tree-sitter` + `tree-sitter-python` grammar (parse source file into a Rust AST).
4.  **Execution Engine:** Custom Rust tree-walking interpreter operating on the Rust AST.
5.  **Caching:** Simple Rust `HashMap<ExpressionID, pyo3::Py<pyo3::PyAny>>` (in-memory only). `ExpressionID` needs to uniquely identify an AST node (e.g., hash of path from root, or simple node ID). Store Python object references managed by `pyo3`.
6.  **State:** Simple `HashMap<String, pyo3::Py<pyo3::PyAny>>` for variable scope (in-memory only).
7.  **UI:** `ratatui` + `crossterm` backend (basic read-only display).
8.  **File Watching (Optional but helpful):** `notify` crate (or similar) to trigger re-evaluation on save.

**MVP Feature Set (Extremely Limited Python Subset):**

1.  **Literals:** Integers, possibly strings.
2.  **Variable Assignment:** `var_name = expression`.
3.  **Variable Reference:** Using `var_name` in expressions.
4.  **Binary Operations:** Just addition (`+`) to start.
5.  **No:** Functions, classes, imports, loops, conditionals, lists, dicts, methods, etc.

**Core Interpreter/Engine Logic (v0.0.0 Sketch):**

```rust
// Simplified Concept
struct GardenEngine {
    // AST representation derived from tree-sitter
    ast: AstMap, // Maps NodeID -> AstNodeData
    // In-memory cache of computed Python objects
    cache: HashMap<NodeID, Py<PyAny>>,
    // In-memory variable scope
    scope: HashMap<String, Py<PyAny>>,
    // Source code lines for UI
    source_lines: Vec<String>,
}

impl GardenEngine {
    fn evaluate(&mut self, node_id: NodeID, py: Python) -> PyResult<Py<PyAny>> {
        // 1. Check Cache
        if let Some(cached_obj) = self.cache.get(&node_id) {
            return Ok(cached_obj.clone_ref(py));
        }

        // 2. Get AST Node data
        let node_data = self.ast.get(&node_id).unwrap(); // Simplified unwrap

        // 3. Execute based on node type (minimal subset)
        let result_obj = match node_data {
            AstNodeData::Literal(value) => {
                // Create PyObject for the literal using pyo3
                value.to_object(py) // Simplified
            }
            AstNodeData::Variable(name) => {
                // Look up in scope
                self.scope.get(name).cloned().unwrap_or_else(|| py.None()) // Handle errors better later
            }
            AstNodeData::Assignment { target, value_node } => {
                let value_pyobj = self.evaluate(*value_node, py)?;
                self.scope.insert(target.clone(), value_pyobj.clone_ref(py));
                // Assignment expression itself might return the value or None
                value_pyobj
            }
            AstNodeData::BinaryOp { op: '+', left, right } => {
                let left_pyobj = self.evaluate(*left, py)?;
                let right_pyobj = self.evaluate(*right, py)?;
                // Use pyo3 to call Python's C-API for addition
                // (Needs acquiring GIL, error handling)
                let sum_obj = left_pyobj.call_method1(py, "__add__", (right_pyobj,))?;
                 sum_obj
            }
            // ... other minimal cases or panic/error
             _ => panic!("Unsupported node type")
        };

        // 4. Update Cache (clone PyObject reference for cache)
        self.cache.insert(node_id, result_obj.clone_ref(py));

        Ok(result_obj)
    }

     // Function to traverse AST and populate cache/scope initially
     fn run_initial_evaluation(&mut self, py: Python) {
        // Iterate through relevant top-level nodes in AST
        for node_id in self.ast.get_execution_order() { // Needs defining
           let _ = self.evaluate(node_id, py); // Ignore result, just populate cache/scope
        }
     }

     // Function to get cached values for the UI
     fn get_display_values(&self, py: Python) -> HashMap<NodeID, String> {
         self.cache
             .iter()
             .map(|(id, obj)| (*id, obj.bind(py).repr().unwrap().to_string())) // Simplified repr
             .collect()
     }
}

```

**TUI (v0.0.0 Sketch):**

*   Displays the lines of the source Python file.
*   Next to each line containing a tracked expression (assignments, binary ops in this MVP), display the latest value fetched from the `GardenEngine`'s cache (`get_display_values`). Uses `repr()` for simplicity.
*   Refreshes the display when the `GardenEngine` indicates its state has changed (e.g., after a file save and re-evaluation).

**Workflow v0.0.0:**

1.  `garden-py test.py`
2.  The program starts, parses `test.py` using Tree-sitter.
3.  The `GardenEngine` performs an initial walk, evaluating the minimal subset and populating the *in-memory* cache and scope.
4.  The `ratatui` TUI launches, displaying the source lines and the initial values fetched from the engine's cache.
5.  The user edits `test.py` in a *separate editor* and saves.
6.  **(With file watching):** The `notify` crate detects the save. The engine clears its cache & scope (simplest invalidation), re-parses, re-runs the initial evaluation, and triggers a UI refresh.
7.  **(Without file watching):** The user has to restart `garden-py test.py` to see changes.
8.  The TUI updates, showing the new values next to the relevant lines.

**Key Exclusions for v0.0.0:**

*   Persistence (no saving cache/state).
*   Most Python features (functions, classes, imports, loops, etc.).
*   Robust error handling.
*   Efficient cache invalidation (just clear everything).
*   Live editing *within* the TUI.
*   Performance optimizations (GIL handling etc.).
*   Complex `PyObject` serialization.

This MVP focuses *purely* on demonstrating the **Engine(AST -> PyObject Cache/Exec -> TUI Update)** cycle for the simplest possible cases, proving the core interaction model is feasible before tackling the immense complexity of full Python support.