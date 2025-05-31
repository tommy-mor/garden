A projectional editor doesn't just solve the UI problem - it dissolves it entirely. Here's why:

## The Text-Based UI Dilemma

In text, you're trying to squeeze values into a medium that wasn't designed for them:

```
let data = fetch("api.com")    // <- where does {"users": [...], "posts": [...]} go?
let count = data.users.length  // <- where does 42 go?
```

Your options are all compromises:
- **Inline comments**: Break file editing, fight with formatters
- **Virtual text overlays**: Limited to text, can't interact, editor-specific
- **Separate panes**: Disconnected from code, requires mental mapping
- **Hover tooltips**: Hidden by default, disappears, blocks code

## The Projectional Editor Solution

In a projectional editor, code isn't text - it's visual nodes. Values aren't annotations - they're **part of the node's visual representation**:

```
┌─────────────────────────────┐
│ data = fetch("api.com")     │
│ ▼ {"users": [...], "pos..." │
└─────────────────────────────┘
```

The node naturally contains both the expression and its value. No fighting for space.

## Why This Changes Everything

**1. Rich Values Are Native**
- Large JSON? Expandable tree view built into the node
- Array of numbers? Inline sparkline
- Image data? Actual thumbnail
- Tabular data? Actual table

**2. Progressive Disclosure**
```
┌──────────────────┐
│ data = fetch(..) │
│ ▼ Object         │  <- Collapsed
└──────────────────┘

┌─────────────────────────┐
│ data = fetch(..)       │
│ ▼ {                    │  <- Expanded
│     users: [10 items]  │
│     posts: [25 items]  │
│   }                    │
└─────────────────────────┘
```

**3. Smart Layout**
The editor's layout engine handles:
- Wrapping long values
- Aligning related nodes
- Showing/hiding values based on zoom level
- Keeping code readable while values are visible

**4. Values Follow Structure**
In nested expressions, values appear exactly where they're computed:
```
┌─────────────────────────────────┐
│ sum = reduce(                   │
│   ┌─────────────────┐           │
│   │ map(nums, ...)  │           │
│   │ → [2,4,6,8]     │           │
│   └─────────────────┘           │
│   add                           │
│ )                               │
│ → 20                            │
└─────────────────────────────────┘
```

## The Fundamental Insight

Text editors are optimized for editing text, not showing live computation. They treat values as second-class citizens that must be squeezed into margins.

Projectional editors are optimized for editing computation. Values are first-class parts of the visual representation. The UI problem disappears because you're not trying to force two separate things (code and values) into one medium (text).

It's like the difference between:
- Writing sheet music and imagining how it sounds
- Using a DAW where you see waveforms and hear audio

Garden needs a medium designed for its vision. Text isn't that medium.