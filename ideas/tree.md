### “Immutable computation trees” -– the core idea

> **One sentence**: *Your entire program is a persistent tree of expression-nodes; each node is an immutable value that knows (a) its code, (b) the cached result of evaluating that code, and (c) pointers to its child-nodes.*

Because the tree structure never mutates **in place**, any change (editing code, clearing a cache entry, toggling a flag) is expressed by *creating a new path* that shares unchanged sub-trees with the old version.
That single rule unlocks fast incremental evaluation, perfect undo/redo, time-travel, rock-solid concurrency, and makes the UI trivially “expand-to-see-value”.

---

## 1. Anatomy of a node

```
struct Node {
    Hash id;                  // content-hash of `code` + children → stable identity
    String code;              // source snippet for this node (e.g. "add(x, y)")
    Vec<NodePtr> children;    // references to child nodes (persistent pointers)
    Option<Value> cached;     // last evaluated result, if any
    Metadata meta;            // timestamps, editor hints, etc.
}
```

* **Immutability**: `Node` is never mutated after creation.
* **Identity**: `id` is a *structural* hash, so the same sub-tree reused elsewhere
  automatically de-duplicates.
* **Persistence sharing**: two program versions that differ in one leaf share
  everything else, giving Git-style cheap branching for free.

---

## 2. Editing == building a new tree

Imagine the file `math.expr`:

```
1: let x = 2
2: let y = 3
3: x + y
```

Parsing it yields

```
( program
  ( let "x" 2 )
  ( let "y" 3 )
  ( + (var "x") (var "y") )
)
```

*Changing `y` to `30` creates an **entirely new** node for `literal 30`, plus a
new `let-y` node, plus a new root — but line 1’s sub-tree and the `+` node can
be *reused verbatim* because they’re content-identical. A three-pointer change,
not a full rewrite.*

---

## 3. Evaluation algorithm

1. **Memoized DFS**: walk the tree depth-first.
2. For each node, ask *“Do all children have cached values and does `cached`
   exist for me?”*

   * Yes → return `cached`.
   * No → compute `value = eval(code, child_values)`

     * store `value` in a new `Node` (immutable, remember)
     * bubble the new pointer upward.

Because unchanged sub-trees keep their hashes, 90-95 % of a typical re-run
instantly hits the cache. You only pay for the slice you edited.

---

## 4. How the UI becomes obvious

* **Tree view**: The editor just paints the parse tree. Each node is
  expandable → shows `cached` in a gutter or tooltip.
* **Inline evaluation**: Click a node → “re-run *this* sub-tree”, get a fresh
  value, drop the old pointer on that path, and let memoisation do the rest.
* **Time-travel slider**: every save is a root pointer; move the slider to
  swap roots and instantly see the entire program *as-of-then* (values
  included).
* **Diffs**: two roots → run a structural diff (`==` on hashes) and you know
  exactly which sub-trees changed and which outputs changed.


## 5. Tiny pseudo-Garden example

```rust
// pseudo-syntax
node root = block! {
    let albums = http_get!("https://api.spotify.com/v1/me/albums");
    map(albums, album -> album.name)
};

// expand a subtree in the UI:
hover(root.children[1]);      // shows cached vector of album names

// edit map body:
let new_root = root.replace_at(
    path = [1, 0],            // go into map → lambda body
    new_code = `album.artist` // now we want the artist
);
evaluate(new_root);           // only the map -> lambda subtree recomputes
```

---

## 6. Why this feels like a breakthrough for you

| Goal in **Garden**            | How immutable trees satisfy it                                   |
| ----------------------------- | ---------------------------------------------------------------- |
| Merge edit- and run-time      | Every edit *is* a new run-time object; values live right there.  |
| Value-first, examples > types | Values are cached on nodes; examples literally sit in the tree.  |
| Reactive feedback             | Recompute minimal affected slice, render diff instantly.         |
| Implementation simplicity     | Storage = hash-map of blobs, evaluation = memoised DFS.          |
| AI-native                     | Each node is a perfect context chunk: `(code, children, value)`. |

---

### Next experiment (low lift, high insight)

1. **Prototype parser** that turns a toy Lisp-ish file into immutable `Rc<Node>`.
2. **Evaluator**: DFS memoiser + tiny `eval` dispatch (`+`, `let`, call-by-name).
3. **CLI UI**: `garden run file.expr` → prints an ASCII tree, each line
   appended with cached value. Edit file, re-run, watch which lines re-eval.

You’ll feel the *click* fast. After you trust the model, layer on LSP hover,
then a simple Tauri/React tree-viewer.

---

Does this picture line up with what you were hoping Garden could become? What parts still feel fuzzy?
