Perfect — that’s a great place to start. If Garden is fundamentally about expressions whose values are cached and shown live, then **data literals** are the foundation. So let’s define the first layer of Garden’s syntax: **data literals that are first-class expressions**.

---

## 🌱 Garden Data Literal Syntax

### ✅ Supported Literal Forms (v0)

#### 1. **Maps / Dicts**
```rust
// simple string keys
{ name: "Tommy", role: "founder" }

// mixed-type keys
{ ["name", 42]: "Tommy", [true, false]: "duality" }

// string/int keys that are valid idents
{ user_id: 123, count: 4 }
```

✅ **Composite keys** allowed  
✅ **Keys can be any literal or expression**  
✅ Internally desugars into: `Map<Value, Value>`

#### 2. **Vectors / Lists**
```rust
[1, 2, 3, 4]
["hello", "world"]
[true, 42, {nested: "map"}]
```

✅ Always ordered  
✅ Corresponds to `Vec<Value>` or `List<Value>`

#### 3. **Sets**
```rust
#{1, 2, 3}
#{"a", "b", "c"}
```

✅ Unordered  
✅ Must contain unique elements  
✅ Corresponds to `Set<Value>`

---

## 🌳 Example Garden Data Literal Program

Let's imagine a `.expr` file:

```rust
let user = { id: 123, name: "Tommy", roles: ["admin", "dev"] }

let config = {
  ["env", "dev"]: true,
  ["env", "prod"]: false
}

let id_set = #{1, 2, 3}

let team = [
  {name: "Alice", age: 30},
  {name: "Bob", age: 28}
]
```

And next to that in the UI, `.value` would cache the full evaluated form. You could hover/click to drill into any part of the value.