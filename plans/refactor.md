### Maintaining a **forward graph** (parent → child) **and a reverse graph** (child → parent) is the classic, battle-tested way to make a live-coding spreadsheet engine work.

Here’s how it plays out in practice, with the trade-offs spelled out.

| Goal                                                                       | Forward graph helps with…                                     | Reverse graph helps with…                                                                                               |
| -------------------------------------------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| **Topological order** – knowing which nodes you can safely evaluate next   | Walking `parent → children` gives you the natural eval order. | (Not needed)                                                                                                            |
| **Dirty-propagation** – “x changed, who depends on it?”                    | You’d have to scan **all** parents to find references → O(N). | Constant-time lookup: `reverse[x]` yields immediate dependents; walk recursively.                                       |
| **Garbage-collecting dead code** – nodes no longer reachable from any root | Doable but slower (need a mark-and-sweep).                    | Start from roots, walk forward; any node with **in-degree == 0** in the reverse graph and not marked reachable is dead. |
| **Explaining why something changed** (debug-UX)                            | Show the chain `root → … → node`.                             | Show the chain `node ← … ← root`.                                                                                       |

**Net:** keeping both maps buys you O(1) answers to the two questions you ask on every edit:

1. *“Which nodes are now dirty?”* → reverse graph.
2. *“In what order do I recompute them?”* → forward graph.

---

## Implementation sketch (Rust-ish pseudocode)

```rust
struct DepDag {
    /// parent -> { children }
    forward: HashMap<NodeId, SmallVec<[NodeId; 4]>>,
    /// child  -> { parents }
    reverse: HashMap<NodeId, SmallVec<[NodeId; 4]>>,
}
```

### Recording deps during evaluation

```rust
fn eval_node(&mut self, parent: NodeId, child: &Rc<Node>, env: &Env) -> Result<Value, Error> {
    // … evaluate child …
    self.depdag.forward.entry(parent).or_default().push(child.id);
    self.depdag.reverse.entry(child.id).or_default().push(parent);
    // …
}
```

*(Do this only once per evaluation pass; clear the depdag at the start.)*

### Change propagation

```rust
fn mark_dirty(&mut self, changed: NodeId) {
    let mut stack = vec![changed];
    while let Some(id) = stack.pop() {
        if self.dirty.insert(id) {
            if let Some(parents) = self.depdag.reverse.get(&id) {
                stack.extend(parents);
            }
        }
    }
}
```

### Re-evaluation order

1. Collect all `dirty` nodes.
2. **Topologically sort** them using the forward edges (Kahn’s algorithm).
   You already have explicit child lists in each `Node`, so that’s straightforward.

---

## Practical considerations

| Topic                        | Notes                                                                                                                                                                                                                                    |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Memory**                   | Two `SmallVec` lookups per edge. Even with \~50 k nodes this is small (< 10 MB).                                                                                                                                                         |
| **Cycles**                   | Because Garden is a *pure* expression tree you normally can’t create cycles, but indirect symbol references (`let a b`, `let b a`) could. Detect during insertion: if `parent == child` or `reverse walk` hits `parent`, raise an error. |
| **Persistence**              | You don’t actually have to serialize the graphs. Rebuilding them on every file load is cheap (one walk of the AST) and guarantees coherence with the fresh source. Persist the **value cache** only.                                     |
| **Thread safety**            | If/when you parallelise evaluation, put `DepDag` behind an `RwLock` and use immutable `NodeId`s as keys to avoid `Rc` sharing headaches.                                                                                                 |
| **Invalidation granularity** | Dirty-marking transitively will sometimes re-eval more than necessary (e.g., if branch predicates select different paths). That’s acceptable now; you can always add “runtime discovered deps” later like Pluto.jl.                      |

---

## When *not* to store the reverse map

* **Ultra-small graphs** (< 200 nodes) where an O(E) scan is truly trivial.
* **One-shot compilers** where you always re-compute everything anyway.

Garden’s vision is an *interactive* environment where you want **<100 ms** feedback, so the reverse map is absolutely worth the two extra `HashMap`s.

---

### Bottom line

Maintaining both directions is the simplest, most reliable path to spreadsheet-style reactivity:

* **Correct** – no stale values like the `(+ x 3)` bug.
* **Predictable performance** – cost is proportional to actual change set, not whole program size.
* **Great UX hooks** – enables “why did this cell update?” explanations.

If this sounds good, we can dive into a concrete patch set: data-structure definitions, integration points inside `Evaluator`, and a micro-benchmark to confirm the speedup. 🚀
