# klog for Zed

> **[klog](https://klog.jotaen.net)** language support for the [Zed](https://zed.dev) editor — syntax highlighting, smart snippets, hover reports, and inline project breakdowns powered by a built-in LSP.

![Zed Extension](https://img.shields.io/badge/Zed-Extension-blue?logo=zed)
![License](https://img.shields.io/badge/license-MIT-green)
![klog](https://img.shields.io/badge/klog-v7.1-orange)

---

## Features

### Syntax Highlighting

Full Tree-sitter–based syntax highlighting for `.klog` and `.klg` files:

- Date headers (`2026-05-20`) and should-total annotations (`(8h!)`)
- Entry types: timespans (`9:00 - 10:30`), open-ended ranges (`9:00 - ?`), durations (`+1h30m`, `-30m`)
- Tags (`#project=Alpha`, `#coding`)
- Comments (`# this is a comment`)
- Record summaries

### Document Outline

The outline panel (`⌘⇧O`) lists all date records as navigable symbols, making it easy to jump between days in large files.

### Snippets

| Prefix | Expands to |
| :--- | :--- |
| `record` | Full record block with today's date + summary placeholder |
| `date` / `20` | Today's date (`YYYY-MM-DD`) |
| `time` | Current time (`HH:MM`) |
| `ts` / `timespan` | Timespan entry (`HH:MM - HH:MM`) |
| `tsoe` / `timespan-open-ended` | Open-ended timespan (`HH:MM - ?`) |
| `st` / `shouldtotal` | Should-total annotation (`(Xh!)`) |

### Hover Reports (LSP)

Hover over any element to get an instant report powered by the `klog` CLI:

#### Date header hover
```
### Day Report: 2026-05-20

| Metric | Value  |
| :----- | :----- |
| Total  | 6h     |
| Should | 8h!    |
| Diff   | -2h    |
```

#### Tag hover (`#coding`, `#meeting`, …)
```
### Tag Report: #coding

| Date        | Time |
| :---------- | :--- |
| 2026 May 19 | 3h   |
| 2026 May 20 | 5h   |
| **Total**   | **8h** |
```

#### Project tag hover (`#project=Alpha`)
```
### Tag Report: #project=Alpha

| Date        | Time   |
| :---------- | :----- |
| 2026 May 20 | 1h30m  |
| **Total**   | **1h30m** |
```

### Inline Project Breakdown (Code Lens / Inlay Hints)

When your file contains `#project=Value` tags, an inline breakdown appears at the very top of the file showing total time per project — no need to open a terminal:

```
#project=Alpha │ 1h30m
#project=Beta  │ 3h30m
Total          │ 5h
```

> **Note:** Enable inlay hints or code lenses in your Zed settings to see this:
> ```json
> { "inlay_hints": { "enabled": true } }
> ```

### Zed Tasks

Run common `klog` commands directly from the editor via the task runner (`⌘⇧P` → **task: spawn**):

| Task | Command |
| :--- | :--- |
| `klog: Total Time` | `klog total <current file>` |
| `klog: Report` | `klog report <current file>` |
| `klog: Today` | `klog today <current file>` |
| `klog: Tags` | `klog tags <current file>` |
| `klog: Pretty Print` | `klog print <current file>` |
| `klog: Start Open-Ended Range` | `klog start <current file>` |
| `klog: Stop Open-Ended Range` | `klog stop <current file>` |

---

## Requirements

- [Zed](https://zed.dev) editor
- [klog CLI](https://klog.jotaen.net) v7.1+ available in your `$PATH`

Install klog via Homebrew:

```bash
brew install jotaen/klog/klog
```

Or download a binary from the [klog releases page](https://github.com/jotaen/klog/releases).

---

## Installation

### From the Zed Extension Marketplace

1. Open Zed.
2. Press `⌘⇧P` → **zed: extensions**.
3. Search for **klog** and click **Install**.

### As a Dev Extension (from source)

1. Clone this repository:
   ```bash
   git clone --recurse-submodules https://github.com/QuentinRob/klog-zed-extension.git
   ```
2. Build and install the LSP binary (requires [Rust](https://rustup.rs)):
   ```bash
   cd klog-zed-extension
   cargo install --path .
   ```
3. Open Zed, press `⌘⇧P` → **zed: install dev extension**, and select the cloned directory.

---

## Usage

Open any `.klog` or `.klg` file and start writing. Here is a quick example:

```klog
2026-05-20 (8h!)
Sprint planning + focused work #coding #project=Alpha

    9:00 - 10:30 Sprint planning #meeting
    +1h30m Deep work on LSP #coding #project=Alpha
    -30m Coffee break
    12:00 - 15:30 Afternoon implementation #coding #project=Beta
    15:30 - ? Open-ended session #coding
```

- **Hover** `2026-05-20` → day report table
- **Hover** `#coding` → tag time breakdown table
- **Hover** `#project=Alpha` → project-specific breakdown
- **Line 0 inline hint** → total time per project value

---

## Architecture

| Component | Language | Purpose |
| :--- | :--- | :--- |
| `src/lib.rs` | Rust (Wasm) | Zed extension bridge — discovers and launches `klog-lsp` |
| `src/bin/klog-lsp.rs` | Rust (native) | JSON-RPC LSP server — implements hover, code lens, inlay hints |
| `languages/klog/` | TOML / S-expr | Language config, Tree-sitter highlight & outline queries |
| `snippets/klog.json` | JSON | Editor snippets |
| `grammars/klog` | Git submodule | [tree-sitter-klog](https://github.com/Ansimorph/tree-sitter-klog) grammar |

The LSP server (`klog-lsp`) is a **separate native binary** that runs on the host machine. It shells out to the `klog` CLI to compute reports and returns them as Markdown over the JSON-RPC protocol. The Zed Wasm extension acts purely as a thin launcher.

---

## Development

```bash
# Clone with submodule
git clone --recurse-submodules https://github.com/QuentinRob/klog-zed-extension.git
cd klog-zed-extension

# Build and install the LSP binary to ~/.cargo/bin/klog-lsp
cargo install --path .

# Install the extension as a dev extension in Zed
# ⌘⇧P → zed: install dev extension → select this directory

# After making changes to the LSP, reinstall and restart the language server
cargo install --path .
# ⌘⇧P → lsp: restart language server
```

LSP logs are written to `/tmp/klog-lsp.log` for debugging.

---

## Contributing

Contributions are welcome! Please open an issue or pull request on [GitHub](https://github.com/QuentinRob/klog-zed-extension).

---

## Credits

- [klog](https://klog.jotaen.net) by [jotaen](https://github.com/jotaen) — the plain-text time-tracking format and CLI this extension is built around.
- [tree-sitter-klog](https://github.com/Ansimorph/tree-sitter-klog) by [Ansimorph](https://github.com/Ansimorph) — the Tree-sitter grammar used for syntax highlighting.

---

## License

MIT
