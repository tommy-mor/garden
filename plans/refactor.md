You've hit on some very astute observations, and your intuition is leading you in the right direction for a more robust and elegant system. The current approach, while functional for the initial proof-of-concept, has a few architectural growing pains that a refactor can address.

You're absolutely right:

1.  **One Cache is Enough:** The `NodeCache` (keyed by `NodeId`) is the fundamental one. The older `ValueCache` (keyed by symbol name) can be subsumed or its purpose re-evaluated. The canonical value of an expression *is* what `NodeCache` should store.
2.  **`ExprAst` can be Bypassed:** The parser can be made to directly construct `Rc<Node>` instances. This simplifies the pipeline.
3.  **Lexical Scope:** The global `context: IndexMap<String, Value>` is a simplification. Proper lexical scope (e.g., for function arguments, `let` bindings) requires environments that are passed down and extended. `def` as a global mutation is also a bit crude for a more functional language.
4.  **Dependency Tracking & Invalidation:** `mark_dependents_changed` is a manual, "push-style" approach. When `X` changes, you tell all known dependents of `X` to re-evaluate. A more "pull-style" or graph-aware approach would be more robust. Invalidation *should* "walk every path" implicitly or explicitly.

**The Central Dependency Graph Idea**

Yes, you're correct. The core of this system *is* a dependency graph.

*   **Nodes of the graph:** These are your `Node` instances (expressions).
*   **Edges of the graph:**
    *   **Structural Dependencies:** An expression node `(op A B)` depends on its children `A` and `B`. If `A` or `B`'s value changes, `(op A B)` might need to re-evaluate. This is naturally handled by the tree structure and recursive evaluation.
    *   **Symbolic Dependencies:** An expression `(+ x 1)` depends on the current value bound to the symbol `x`. If the binding of `x` changes, `(+ x 1)` needs to re-evaluate. This is where things can get more sophisticated.

**How to Refactor Towards This:**

Let's outline a refactor plan incorporating these insights.

**Phase 1: Core Data Structures & Parsing**

1.  **Simplify `Node` Structure:**
    *   Remove `cached_value` from the `Node` struct itself. The cache will be external.
    *   Keep `id`, `kind`, `code_snippet`, `children`, `metadata`.

2.  **The One True Cache (`EvaluationCache`):**
    *   `struct EvaluationCache { cache: HashMap<NodeId, Result<Value, Error>>, timestamps: HashMap<NodeId, Instant> }` (or some generation counter instead of `Instant`).
    *   This stores the last computed value for any `NodeId`. Timestamps/generation counts help determine if a cached value is truly "current" relative to its dependencies.

3.  **Environment for Lexical Scope (`Env`):**
    *   `struct Env<'parent> { bindings: HashMap<String, NodeId>, parent: Option<&'parent Env<'parent>> }`
        *   `bindings`: Maps symbol names (e.g., "x") to the `NodeId` of the expression node that *defines* or *provides* its value (e.g., the `NodeId` of the `10` in `(let x 10)`, or the `NodeId` of the value part of a `(def x ...)`).
        *   `parent`: For lexical scoping. Allows chained lookups.
    *   `Env` needs methods like `resolve(symbol_name) -> Option<NodeId>` and `extend(new_bindings) -> Env`. Consider using persistent immutable data structures (like `im::HashMap`) for `Env` if performance with many nested scopes becomes an issue, to make `extend` cheaper.

4.  **Parser Builds `Node`s Directly:**
    *   Modify `src/parser.rs` and `expr.pest`.
    *   The parser functions (like `parse_expr`) should return `Result<Rc<Node>, Error>`.
    *   The `NodeId` (structural hash) is computed as each `Node` is created.

**Phase 2: The Evaluation Engine (`Evaluator`)**

1.  **`struct Evaluator` (or `Runtime`):**
    *   Holds the `EvaluationCache`.
    *   Possibly a `nodes: HashMap<NodeId, Rc<Node>>` to easily retrieve any node by ID if not always traversing from roots. (This might be redundant if roots are always known and tree is self-contained).

2.  **`eval_node(&self, node: &Rc<Node>, env: &Env) -> Result<Value, Error>` method:**
    *   **Key Idea for Symbolic Dependencies:** When `eval_node` encounters a `NodeKind::Symbol(name)`:
        1.  It calls `env.resolve(name)` to get the `defining_node_id`.
        2.  It then recursively calls `self.eval_node(get_node(defining_node_id), env)` to get the value of that symbol.
    *   **Cache Interaction:**
        1.  Before computing, check `self.cache.get(&node.id)`.
        2.  **Crucial step for staleness:**
            *   If found, we must verify it's not stale. A value is stale if any of its *dependencies* have changed *since this node was last evaluated*.
            *   **Structural dependencies (children):** Recursively evaluate children. If any child's value differs from what was used when the current node's cached value was computed, then the current node is stale. (Timestamps/generation counters help here: if a child's timestamp is newer than the parent's cached value's timestamp, parent is stale).
            *   **Symbolic dependencies:** If `eval_node` is evaluating `(+ x 1)` and `x` resolves to `node_X`, it evaluates `node_X`. If `node_X`'s value changes, then `(+ x 1)` is recomputed.
        3.  If stale or not found, compute the value:
            *   For literals: return value.
            *   For operations: recursively call `eval_node` on children, then perform the operation.
            *   For `let name = value_expr in body_expr`:
                *   Evaluate `value_expr` in current `env` -> `val`.
                *   Get `value_expr_node_id`.
                *   Create `new_env = env.extend({name: value_expr_node_id})`.
                *   Evaluate `body_expr` in `new_env`.
            *   For `def name value_expr` (if still global):
                *   Evaluate `value_expr` -> `val`.
                *   Get `value_expr_node_id`.
                *   Update a global/top-level `Env` mapping `name` to `value_expr_node_id`.
                *   **Invalidation Impact:** This change in the global `Env` *will implicitly invalidate* other nodes that read `name` because `env.resolve(name)` will now point to a potentially different (or re-evaluated) `NodeId`.
        4.  Store the result in `self.cache.insert(node.id, result, current_timestamp)`.

**Dependency Graph Traversal & Invalidation - The "Pull" Model:**

With this refactor, invalidation becomes more of a "pull" model driven by the `eval_node` traversal:

*   You don't *explicitly* walk the graph to mark nodes "dirty".
*   When `eval_node` is called for a node `N`:
    *   It first asks its dependencies (children, symbols it uses) for their current values by recursively calling `eval_node` on them.
    *   If a dependency `D` recomputes and its value changes (or its timestamp updates), then `N` "sees" this new value.
    *   `N` can then compare this new dependency value (or its timestamp) against what it used last time (if this info is stored alongside the cached value of N, or inferred by comparing the dependency's new timestamp to N's cache timestamp).
    *   If a dependency changed, `N` recomputes itself.
*   **Structural hashing is still key:** If a low-level expression's *code* changes, it gets a new `NodeId`, its parent gets a new `NodeId` (because a child's ID changed), and so on up the tree. This automatically "invalidates" by cache miss.
*   The `Env` chain and `eval_node`'s recursive nature for symbols handle the propagation of changes through variable bindings.

**File Watching and Re-evaluation:**

1.  On file change:
    *   Parse the entire file into a new `Vec<Rc<Node>>` (these are the top-level root expressions).
    *   Create a fresh top-level `Env`.
    *   For each root node in the `Vec`:
        *   Call `evaluator.eval_node(root_node, &top_level_env)`.
        *   If `root_node` was a `def`, `eval_node` (or a helper) updates `top_level_env` for subsequent top-level expressions.
2.  The `EvaluationCache` persists across runs. Nodes that are structurally identical to their previous versions (same `NodeId`) will first check the cache. Their evaluation will then "pull" on their dependencies.

**Benefits of this Refactor:**

*   **Single Source of Truth for Cache:** Cleaner.
*   **No `ExprAst`:** Simpler pipeline.
*   **Proper Lexical Scope:** `Env` enables this naturally.
*   **Cleaner Dependency Handling:** Evaluation inherently traverses the dependency graph. Explicit "marking" of dependents for symbols is reduced/eliminated because symbols resolve to `NodeId`s whose values are fetched on demand. If a `def` changes what `NodeId` a symbol points to, users of that symbol will naturally pick up the new definition during their own `eval_node` call.
*   **Robust Invalidation:** Driven by structural hashing and on-demand evaluation through the `Env`.

This is a significant refactor, but it sets up a much more robust and extensible architecture. It directly addresses the concerns you raised and aligns the system more closely with how modern reactive systems or functional language interpreters with memoization often work.