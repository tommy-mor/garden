Okay, based on your vision and the current state of `src/main.rs`, here's a prioritized list of what needs to be implemented next to move significantly closer to the Garden ideal:

**Core Foundation - Phase 1 (Getting the Immutable Tree & Basic Caching)**

1.  **Define the `Node` Structure (as per `ideas/tree.md`):**
    *   **What:** Create the Rust struct for a `Node`. This is the absolute cornerstone.
        ```rust
        // Rough idea
        use std::rc::Rc; // Or Arc for thread-safety if planning for concurrency early
        use std::collections::HashMap; // For metadata

        type NodeId = [u8; 32]; // For a 256-bit hash (e.g., Blake3)

        #[derive(Debug, Clone, PartialEq)] // PartialEq for values, Node might need custom Eq/Hash
        pub enum Value { /* ... as before ... */ }

        #[derive(Debug, Clone)] // May not be cloneable if it has Rc<RefCell<...>> for cached_value
        pub struct Node {
            id: NodeId,
            code_snippet: String,         // The source text of this node
            children: Vec<Rc<Node>>,
            // How the node itself is evaluated - e.g. is it a literal, a symbol, an operation
            kind: NodeKind, // You'll need to define NodeKind (e.g., Literal, Symbol, Call, LetBinding)
            cached_value: Option<Value>,  // Or Option<Result<Value, Error>>
            metadata: HashMap<String, String>, // For source location, timestamps etc.
        }
        ```
    *   **Why:** This is the fundamental data structure that will hold the code, its children, and its cached value. Without it, the rest of the vision can't be built.
    *   **Consider:** How `NodeId` will be generated (see next point).

2.  **Implement Structural Hashing for `NodeId`:**
    *   **What:** Develop a function that takes the constituent parts of a `Node` (its `kind`, `code_snippet`, and the `id`s of its children) and computes a stable, content-based hash (e.g., using Blake3).
    *   **Why:** This `NodeId` is critical for:
        *   Efficiently identifying identical sub-trees.
        *   Serving as the key for the value cache.
        *   Enabling cheap diffing between tree versions.
    *   **Action:** Modify the parser or have a post-parsing step to transform `ExprAst` (or parse directly) into this `Rc<Node>` tree, calculating `NodeId`s for each node, likely bottom-up.

3.  **Create the Central `ValueCache`:**
    *   **What:** A global or context-passed `HashMap<NodeId, Result<Value, Error>>`.
        ```rust
        //  In your main state or context
        //  use std::sync::{Arc, RwLock}; // If you plan for the cache to be shared across threads
        let value_cache: HashMap<NodeId, Result<Value, Error>>;
        ```
    *   **Why:** This is where the results of node evaluations will be stored and retrieved, keyed by their unique `NodeId`.

4.  **Revamp the `eval` Function to be "Reactive" (Memoized DFS):**
    *   **What:**
        *   `eval` should now take an `Rc<Node>` and a mutable reference to the `ValueCache`.
        *   **Logic:**
            1.  For a given `node`, check if `value_cache.contains_key(&node.id)`.
            2.  If yes, return the cached `Result<Value, Error>`.
            3.  If no:
                *   Recursively call `eval` for all `node.children`.
                *   Once children's values are obtained, perform the operation defined by `node.kind` (e.g., if `node.kind` is `Add`, sum the children's values).
                *   Store the computed `Result<Value, Error>` in `value_cache.insert(node.id, result)`.
                *   Return the result.
        *   The current `context: &mut IndexMap<String, Value>` for `def` needs to be rethought. Symbols might resolve to `NodeId`s of their defining expressions, or `def` nodes might interact with a separate symbol table that maps names to `NodeId`s.
    *   **Why:** This implements the core incremental computation. Only nodes not in the cache (or invalidated) will actually recompute.
    *   **Async consideration:** If `Value` can be the result of an async operation (like `http.get`), `eval` needs to remain `async`, and `ValueCache` might store `Result<Value, Error>` where `Value` is the resolved future.

**Proof of Concept - Phase 2 (Making it Tangible)**

5.  **Implement the File-Watching CLI (`garden watch` as per `ideas/cli.md`):**
    *   **What:**
        *   Use a crate like `notify` to watch a `.expr` file.
        *   On file save:
            *   Re-parse the entire file into a *new* root `Rc<Node>`. (Structural hashing is key here: unchanged parts of the code will produce nodes with the *same IDs* as before, so they'll hit the cache).
            *   Call the new `eval(new_root_node, &mut value_cache)` for the new tree's root.
            *   Pretty-print the (selected parts of) the tree, showing the `code_snippet` and its `cached_value` for each node (or just changed nodes).
    *   **Why:** This is the first user-facing deliverable that will prove the core reactive loop. Seeing values update instantly next to code will be the "aha!" moment.

6.  **Basic Persistence for `ValueCache`:**
    *   **What:** On startup, try to load the `ValueCache` from a file (e.g., a RON or JSON file that maps stringified `NodeId`s to `Value`s). On clean shutdown (or periodically), save it.
    *   **Why:** To make the "values live with the code" truly persistent across sessions.
    *   **Example from project:** `values.ron` hints at this, but it should be driven by `NodeId`.

**Consolidating & Expanding - Phase 3**

7.  **Source Mapping:**
    *   **What:** Ensure that each `Node` in the tree stores its source location (file, line, column start/end) in its `metadata`. The parser needs to capture this.
    *   **Why:** Essential for correctly displaying values next to their corresponding code in any UI or editor integration.

8.  **Language Refinements & Basic Function Definitions:**
    *   **What:**
        *   Standardize `let` vs. `def`. `let` for local lexical bindings is generally preferred for functional languages.
        *   Implement user-defined functions (e.g., a `fn` or `lambda` special form). Evaluating a call to a user-defined function would involve creating a new lexical scope for its parameters, binding arguments to them, and then evaluating the function's body. The `Node` for the function call would cache the *result* of the call.
    *   **Why:** To make the language expressive enough for more complex examples and to start building abstractions.

**What to put on hold for now (but keep in mind for later):**

*   Full nREPL server.
*   Advanced UI (Tauri/Web).
*   RocksDB integration (start with simple file persistence for the cache).
*   Sophisticated error reporting and visualization (basic `Result` in cache is a start).
*   LSP server (the CLI output can serve as a simpler "UI" for now).

This path focuses on building the core engine of immutable trees, hashing, caching, and reactivity first, then immediately applying it to a simple file-watching CLI to demonstrate its power.