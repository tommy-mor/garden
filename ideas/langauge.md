### 🌱 **Garden in a nutshell**

| Aspect | What Garden **is** | How it **differs** from Clojure |
|--------|-------------------|---------------------------------|
| **Philosophy** | *Value-first, memory-bearing* Lisp. Every named expression keeps its last value on disk and shows it live in your editor. | Clojure is *function-first, forgetful.* REPL values vanish once you restart. |
| **Syntax** | S-expressions & keywords, but stripped to the essentials—no `ns`, no reader macros, no JVM interop baggage. | Clojure’s surface is larger (metadata, dispatch macros, EDN literals, etc.). |
| **Runtime** | A tiny interpreter written in **Rust**. Hot-reload; side-effect ops are just host “opcodes.” | Clojure compiles to JVM byte-code; heavy startup, large dependency graph. |
| **Persistence layer** | RocksDB / RON files keyed by *structural path* so values survive edits and restarts. | Clojure has no built-in persistence; you’d add Datomic or Atomics yourself. |
| **Extensibility** | **Babashka pod protocol** out of the box → call Python, shell, Postgres, etc., without FFI pain. | Clojure uses JVM interop; Babashka pods are possible but external. |
| **Type stance** | “Examples > types” – optional shape inference from cached values; you generalize later. | Clojure is dynamically typed with spec/clj-type libraries for optional checks. |
| **Editor story** | Any LSP/nREPL client; inline eval uses the **scope-cache resolver** you and I just specced. | Clojure relies on nREPL too, but without automatic scope replay. |
| **Goal** | Make programming feel like **tending a living data garden**: code, see, tweak, keep context forever. | Clojure’s goal is “simple, hosted, functional,” not persistent inspection. |

---

#### 👩‍🔧  How it hangs together

1. **Parse** source, stable node IDs.  
2. **Interpret** in Rust:
   - basic Lisp core (`def`, `fn`, `let`, `+`, etc.)
   - side-effect opcodes (`http.get`, `postgres.pod/execute!`) call out through pods.
3. **Cache every binding** (`let`, param) with `(ident, file, line-span)` key.  
4. **nREPL server**:
   - Receives `code + file + line`.
   - Resolves free syms from the cache (nearest + freshest).
   - Evaluates, returns EDN + provenance.
5. **Editor plugin** (Emacs/CIDER, VS Code, etc.) shows the value inline.

---

#### 🚫  What Garden is **not**

- **Not** a full Clojure clone—no multimethods, protocols, or macro tower (yet).  
- **Not** a compiler—speed comes from caching, not JIT.  
- **Not** tied to the JVM—Rust native binary, starts in < 100 ms.

---

**Tag-line:**  
> *“Garden is the Lisp where your code remembers its own output.”* 🌿