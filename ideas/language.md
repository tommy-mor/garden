Here’s a cohesive, high-resolution description of **Garden** as a **language**, **runtime**, and **development platform**—distilling its essence and ambition:

---

# 🌱 Garden  
*A runtime-native language, interactive interpreter, and projectional development platform where code is grown, not just written.*

---

## 🧠 The Language

Garden is a **Rust-like, Clojure-inspired expression language** designed to treat values as first-class citizens, not second-order effects. It is:

- **Garbage collected** – Memory management is automatic and safe.
- **Block-scoped with named paths** – Every construct (e.g. loops, functions) can be *named*, giving stable identity for caching and tracking.
- **Syntax: Clean and readable** – Inspired by Rust’s popularity and ergonomics, but with optional semicolons, optional return keywords, and expression finality.
- **No hard types** – Instead of a traditional type system, Garden builds an **example-driven value system**—type arises from runtime behavior.
- **Partial evaluation by design** – Expressions can be frozen, deferred, or memoized.

### Example

```rune
let user = {
  name: "Karen",
  age: 27
};

for each_user in db.users {
  let greeting = "Hello, " + each_user.name;
  log(greeting);
}
```

Each expression (e.g. `user`, `greeting`, `log(...)`) gets a **structural path** like:
```
["root", "user"]
["root", "loop#each_user", 3, "greeting"]
```

---

## ⚙️ The Runtime

Garden’s runtime is **a structural, reactive, value-aware interpreter**. It is:

- **Built in Rust** – Fast, safe.
- **Structural path indexed** – Every expression is mapped to a stable path in the tree.
- **Caching layer** – Values are stored in `.value` files, auto-invalidated when inputs change.
- **Loggable and replayable** – Side effects like API calls, file writes, and errors are logged per expression path.
- **Extensible via Babashka pods** – Garden uses the Babashka pod protocol to call out to rich, cross-language functionality.

The runtime doesn’t just **run** your code. It **remembers**, **reacts**, and **teaches you** what your code *did*.

## 🔮 Summary

**Garden is a new category of development environment**, where:

- Code is not stringly-typed—it is structured, inspectable, and *living*.
- Values aren’t ephemeral—they are stored, rendered, and interactively debugged.
- The act of programming becomes tending to a garden of logic, pruning old leaves, watering roots, and watching new flowers bloom.
