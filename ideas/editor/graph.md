Here's why a graph-based projectional editor isn't just good for Garden - it's probably necessary:

## The Fundamental Mismatch

Garden's core equation "edit time = runtime" assumes computation is a living thing you can see and touch. But text-based code is dead symbols that only come alive during execution. This isn't a tooling problem - it's a fundamental representational mismatch.

## Why Text Breaks Garden's Vision

**1. The Identity Crisis**
```
let x = fetch_data()
let y = process(x)
```
In text, if you rename `x` to `data`, you've destroyed its identity. The cached value of `fetch_data()` is orphaned. But Garden NEEDS stable identity to maintain the illusion that values persist across edits.

**2. The Scope Prison**
Text forces artificial boundaries:
```
let x = 10
let f = fn(x) { x + 1 }  // Different x, same name
```
Garden wants to show you the value of every `x`, but text has already erased the distinction. A graph has no such confusion - each binding is its own node.

**3. The Sharing Problem**
Text makes you name things just to reuse them:
```
let expensive = compute_matrix()
let a = expensive + 1
let b = expensive * 2
```
In a graph, `compute_matrix()` is one node with multiple outgoing edges. One computation, one cached value, naturally shared.

## Why Graphs Are Garden's Natural Form

**1. Computation IS a Graph**
When `z = x + y`, there are edges from `x` and `y` to `+`, and from `+` to `z`. Garden wants to show values flowing along these edges. Text obscures this structure; graphs reveal it.

**2. Values as Node Properties**
In a graph, each node has:
- Its computation (the expression)
- Its cached value
- Its visual representation

No more fighting about where to display values - they're just part of the node.

**3. Time as a Dimension**
Nodes can maintain history. Scrub through time and watch values change. The graph structure remains stable while values flow through it.

**4. Effects as Special Edges**
Pure computations are normal edges. Effects are special edges that show when they last fired, with manual refresh controls. The graph makes the distinction visual and structural.

## The Revolutionary Potential

A graph-based Garden wouldn't just be "code with visible values" - it would be programming in the native structure of computation itself. Like how Excel made spreadsheets visual, Garden could make all computation visual.

This isn't a nice-to-have. Garden's vision - making the invisible visible, collapsing edit and runtime - requires escaping text's linear prison. The graph isn't just a better UI; it's the honest representation of what Garden is trying to show.

Text-based Garden is like trying to build a spreadsheet in a text editor. Possible? Maybe. But why fight the medium when the natural representation is staring us in the face?