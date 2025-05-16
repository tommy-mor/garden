### Why collapse “edit-time” and “run-time” at all?

1. **Tighter feedback loops = tighter cognition**
   Waiting seconds (or minutes) for a compile / deploy cycle forces you to juggle hypotheses in working memory.
   · Inline values let you see the *ground truth* of each expression the moment you hit *save*—your codebase becomes a live notebook instead of a static hope.
   · Rapid confirmation → more experiments per hour → exponential learning curves.

2. **Values are the best documentation**
   Types and comments describe *possibility*; cached values show *reality*.
   When every expression carries its most recent result like a tooltip-backpack you get:

   * concrete examples for onboarding (“Oh, that’s the shape of `UserRow`”)
   * instant outlier spotting (“Why did `total = NaN` here?”)
   * cheaper code review (“Scroll, scroll… yep, those numbers look sane”).

3. **Deterministic replay & debug time-travel**
   By persisting the cache you can:

   * bisect history and reload any past program state without a debugger dance
   * ship a bug report as a single `.expr` + `.value` pair—your collaborator opens the file and *they are already on the failing line*.

4. **AI context windows love concrete state**
   Feeding the model a slice of the cache gives it real examples to condition on, making code-gen far crisper than “here’s a type, guess a value”. Garden is designed to be *AI-native*—merging times collapses the “dry prompt → hallucination” gap.

---

### How Garden actually merges them

> *The mantra: “Every expression has a postal address, every address can cache a value.”*

| Layer                         | What it does                                                                                                                                                                                                                                                                                                           | How it enables the merge                                                       |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| **1. Structural IDs**         | Tree-sitter parses each file; the path from root to node (plus a hash of leading text) becomes a *stable* `ExprID`. Renaming a variable doesn’t break the link.                                                                                                                                                        | Code edits change as little as possible, so caches survive refactors.          |
| **2. Reactive evaluator**     | A tiny Rust interpreter walks the AST. When file-watcher sees a save, it invalidates only the sub-tree whose source changed, then re-executes bottom-up.                                                                                                                                                               | Re-run latency is \~tens of ms for most edits – indistinguishable from typing. |
| **3. Persistent value store** | Results are serialized to human-diffable RON files (`foo.expr.value`) during dev; swap in RocksDB for big projects.                                                                                                                                                                                                    | The values survive editor restarts, git checkouts, even CI.                    |
| **4. UI surfaces**            | *Early CLI* – `garden dev path/to/file.expr` prints the file with color-coded inline results. File is re-rendered on every save.<br> *Planned Web/Tauri UI* – panel showing the AST tree, click an expression → expands cached history graph, rerun/clear buttons. LSP hovers give you the same info in VS Code/Emacs. | You don’t “open a debugger”, you just *look*.                                  |
| **5. Production strip-mode**  | `garden build` copies source, drops `.value` files, freezes cache reads, and compiles to a normal binary or ships interpreter + source.                                                                                                                                                                                | Same semantics, zero dev-only overhead.                                        |

---

### Quick sketch of a dev cycle

```bash
$ garden new hello.expr
```

```rust
// hello.expr
let name = "world";
format!("Hello, {name}!")   // => "Hello, world!"
```

1. `garden dev hello.expr` starts a watcher.
2. You change `name` to `"Garden"`, hit save → CLI instantly re-prints showing the new result.
3. Cache is written to `hello.expr.value`.
4. Commit both files; your teammate pulls and sees the same value without running anything.

---

### Where this lands us

* **Dev loop speed** close to a Lispy REPL, but with *persistent* guarantees and syntax approachable to Rust/TypeScript folks.
* **Runtime parity** – Garden doesn’t invent an “eval server”; the interpreter that shows you values *is* the production interpreter, just running with a cache layer.
* **Platform for Sorter (or anything)** – building an HTTP server in Garden is just composing expressions whose cached values are HTTP responses. Because only changed routes re-compute, hot-reload comes for free.

Think of it as replacing the old two-phase pipeline—

```
[edit text] → [compile] → [run] → [observe] → repeat
```

—with a single breathing organism:

```
[edit text] ⇄ [always-running program whose cells light up as you type]
```

That’s the essence of merging edit-time and run-time—and Garden’s architecture is purpose-built to make it feel obvious.
