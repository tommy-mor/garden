### 🌾 **“Line-Proximity Cache” — a dirt-simple way to give Emacs/CIDER live values without needing `find_enclosing_defn`**

> **Goal**: when you jack-in from Emacs and hit *C-x C-e* on  
> `(postgres.pod/execute! …)` the Garden nREPL server should look up
> a recent value for `req` even though the editor only tells us  
> *file / line / column / sexp*.

---

## 1 · What metadata do we already get?

CIDER (and all the other editors) send an nREPL op that includes:

```clj
{:file "/src/handlers.clj"
 :line 27
 :column 12
 :code "(postgres.pod/execute! ...)"}
```

No AST path, no surrounding defn.

---

## 2 · A pragmatic indexing strategy

| We cache **when** | We store **what** |
|-------------------|-------------------|
| Every time Garden finishes evaluating a var binding (`let`, param, or `def`) | `{ident: "req"  file: ".../handlers.clj"  start: 24  end: 40  ts: 1714095600  value: …}` |

* `start`/`end` are **line numbers** spanned by the *form that created the binding* (easy to grab from the parser).  
* One binding may appear many times (each call). We only keep the most recent N (3? 10?) per ident to bound memory.

Store that in an LSM (RocksDB) keyed by `(ident, file)` so it’s an O(1) lookup.

---

## 3 · Resolving a free symbol during *C-x C-e*

```text
resolve(ident="req", file, line):
  candidates = db.get_all(ident, file)
  if empty      → miss
  score(c) = |c.line_mid - line|            ; distance
            + Wt * seconds_since(c.ts)      ; recency
  return argmin(score)
```

*`line_mid = (start+end)/2`*

This “nearest in text + fresh in time” heuristic is stupid-simple **and it works** because:

* inside a request handler you usually have only one `req` in scope;
* if there are two handlers in the same file, the one that’s literally nearer is the one you’re editing;
* stale snapshots decay out automatically.

When a symbol is *still* ambiguous (score tie) we ask:

```
req: 2 candidates
1) handlers.clj:24–40 (3 s old)   {:query-params {:user-id 7}}
2) handlers.clj:102–133 (8 s old) {:query-params {:user-id 42}}
Pick [1/2]? _
```

(Emacs shows a minibuffer prompt, your choice is cached for the rest of the session.)

---

## 4 · Instrumentation needed in your Rust interpreter

```rust
fn bind(name: &str, value: Value, span: (usize, usize), file: &str) {
    env.insert(name.to_string(), value.clone());
    cache.put(name, file, span, now(), value);    // NEW
}
```

* `span` comes straight from Tree-sitter node APIs (`start_point.row`, `end_point.row`).  
* You already walk every `let` / param, so the extra code is < 40 LOC.

---

## 5 · Runtime when you hit *eval*

1. Parse the incoming sexp (just like today).  
2. Walk its symbol nodes.  
3. For each symbol **not** found in the REPL env:  
   * call `resolve(name, file, line)` → maybe-value  
   * if hit → inject into the eval environment.  
4. Evaluate → return value; also report which symbols were supplied from cache.

> **Example log**
> ```
> 🔄  garden resolved 1 free syms  (req → cache hit, 2 s old, line 26)
> ```

---

## 6 · Limitations & mitigations

| Limitation | Why it’s OK for “hack-mode” |
|------------|----------------------------|
| Renaming `req` to `request` breaks the link | You’ll re-run the handler once; the new ident will be cached immediately. |
| Two vars named `id` in the same function | Distance + recency usually disambiguates; otherwise you choose interactively. |
| Shadowed vars after heavy refactor | Clear-cache hotkey (`C-c M-k`) flushes stale snapshots. |
| Non-file eval (e.g. REPL paste) | Falls back to normal Clojure rules—no cached bindings injected. |

We’re trading  “perfect scope fidelity” for **zero editor changes** and < 1 day of Rust work.

---

## 7 · Quick implementation checklist

1. **Add start/end rows to every binding node** (already available from Tree-sitter).  
2. **Persist snapshots** – RocksDB table:  
   ```text
   key = (ident, file)  →  Vec<Snapshot {span, ts, ron_bytes}>
   ```  
3. **nREPL op** `garden/eval`  
   * parse `:file :line :code`  
   * inject nearest snapshots.  
4. **Emacs glue** – Just reuse `cider-eval-last-sexp` by configuring `cider-nrepl-connection-url` to your Garden port; no code changes!  
5. Optional: minibuffer chooser when >1 candidate.  

Deploy that and you can **poke any sub-expression in situ** and instantly see the value, even deep inside request handlers. 🎉

---

Let me know if you want code snippets for steps 2-3 or details on serialising the snapshot value efficiently (RON vs bincode). 🌿
