Okay, let's describe Garden focusing purely on the developer workflow enhancements enabled by pervasive, structurally-aware value caching. Forget distributed for now; this is about making the *local* coding experience radically better.

---

**Garden: Programming Where Your Code's Values Are Always Visible**

Imagine writing code where you *never* have to hit "run" just to see what a variable holds. Imagine never littering your code with `print` statements just to understand intermediate results. That's the core experience of Garden.

**The Workflow: An Intimate, Instant Conversation with Your Code**

Garden fundamentally changes the development loop by making **every expression remember its last computed value**, keyed to its unique place within your code's structure. Here's what that feels like:

1.  **Type and See:** As you write a line of code, like `let user_ids = fetch_users().map(|u| u.id);`, Garden evaluates it *immediately* in the background. Milliseconds later, you see the actual `user_ids` value appear right beside your code in the Garden interface (a TUI or editor extension). No manual execution step needed.
2.  **Instant Feedback on Change:** You modify an earlier line, maybe changing the `fetch_users()` parameters. Garden instantly detects this, knows which subsequent expressions depend on that value (like `user_ids`), and recomputes *only* those affected parts. You watch the values update down the chain, immediately understanding the impact of your change.
3.  **Effortless Exploration:** Curious about a value from 10 lines ago? Just glance at it or hover over it. Garden shows you the cached value, maybe even when it was computed and what it depended on. There's no need to re-run anything or step through a debugger just to recall a previous state.
4.  **Confident Refactoring:** Because Garden tracks values based on code *structure* (not just variable names), you can rename variables or move code blocks around more confidently. The underlying value cache often remains valid, preserving context and ensuring you haven't subtly broken things.
5.  **Persistent Context:** Close your editor and come back tomorrow? Garden remembers. As long as the code structure relating to a value hasn't changed, the cached value is still there, ready immediately. You pick up right where you left off, with the program's "memory" intact.
6.  **Reduced Debugging Friction:** Many bugs become obvious *as you type* because you see unexpected values immediately. When deeper debugging is needed, you already have a rich history of intermediate values readily available, reducing the need for explicit breakpoints and stepping.

**How it Works (Briefly): Pervasive, Structural Caching**

Garden constantly parses your code (likely using something like Tree-sitter) to understand its structure. Each meaningful expression gets a stable identity based on its position in the syntax tree. As code executes (often automatically in the background), Garden stores the resulting value against that structural identity in a persistent cache (like `.value` files or RocksDB).

**The Core Benefit: Uninterrupted Flow and Understanding**

This pervasive caching fundamentally **removes friction** from the develop-test-debug cycle. Instead of distinct, slow steps, coding in Garden becomes a fluid exploration:

*   You spend less time *running* code and more time *understanding* it.
*   You maintain mental context because the code's state is always visible.
*   You experiment more freely because the cost of seeing the result is near zero.

It transforms programming from executing static instructions into tending a **live system** that constantly reflects its state back to you. The emphasis shifts from "what will this code do when I run it?" to "what is the value of this expression *right now*?"
