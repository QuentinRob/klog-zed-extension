# klog for Zed

> **[klog](https://klog.jotaen.net)** language support for the [Zed](https://zed.dev) editor — syntax highlighting, smart snippets, hover reports, and inline project breakdowns powered by a built-in LSP.

![Zed Extension](https://img.shields.io/badge/Zed-Extension-blue?logo=zed)
![License](https://img.shields.io/badge/license-MIT-green)
![klog](https://img.shields.io/badge/klog-v7.1-orange)

---

## Features

### Syntax Highlighting

Full syntax highlighting for `.klog` and `.klg` files powered by the [tree-sitter-klog](https://github.com/Ansimorph/tree-sitter-klog) grammar:

- Date headers (`2026-05-20`) and should-total annotations (`(8h!)`)
- Entry types: timespans (`9:00 - 10:30`), open-ended ranges (`9:00 - ?`), durations (`+1h30m`, `-30m`)
- Tags (`#project=Alpha`, `#coding`)
- Comments (`# this is a comment`)
- Record summaries

### Document Outline

The outline panel (`⌘⇧O`) lists all date records as navigable symbols, making it easy to jump between days in large files.

### Editor Folding Support

Collapse/fold day records and time entry continuation blocks directly in the editor. Folding is supported natively via Tree-sitter folding queries (`folds.scm`) as well as the LSP (`textDocument/foldingRange`) provider.

### Snippets

| Prefix | Expands to |
| :--- | :--- |
| `record` | Full record block with configurable date, configured day duration target, and summary placeholder (e.g. `2026-05-21 (7h42m!)\nSummary\n    $0`) |
| `date` / `today` / `20` | Dynamic date (configurable placeholder) with the configured day duration target (e.g. `2026-05-21 (7h42m!)` or `YYYY-MM-DD (7h42m!)`) |
| `time` | Current dynamic time (e.g. `10:14`) |
| `ts` / `timespan` | Timespan entry starting at current hour (e.g. `10:00 - 10:00`) |
| `tsoe` / `timespan-open-ended` | Open-ended timespan starting at current time (e.g. `10:14 - ?`) |
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

When your file contains `#project=Value` tags, an inline breakdown appears at the very top of the file showing total time per project, percentage contribution, and an end-of-month projection:

```
Project        │ Total          │ Est. End
───────────────┼────────────────┼────────────────
#project=Alpha │ 2h     (40.0%) │ 12.6d  (60.0%)
#project=Beta  │ 3h     (60.0%) │ 18.9d  (90.0%)
───────────────┼────────────────┼────────────────
Total          │ 5h     (100.0%)│ 31.5d  (150.0%)
```

Both the CodeLens and Hover project reports display each project's percentage contribution relative to the total tracked time, vertically aligned by their parentheses `(` for neat readability.

The `Est. End` projection for projects is automatically calculated by scaling the tracked time for the current month by the ratio of total days in the month to the day of the latest record:
$$\text{Estimated Project Total} = \text{Total So Far} \times \frac{T_{\text{total}}}{D_{\text{latest}}}$$

#### Project Estimation Exemptions
Specific project tags (`ABSCP`, `RTTE`, `RTTS`, `ABSConv`, and `ABSMal`) are kept at their actual tracked value in the `Est. End` projection column instead of being projected by the monthly ratio.

The estimated grand total for the month, however, is calculated as the number of working days in that month multiplied by the configured day duration:
$$\text{Estimated Grand Total} = \text{Working Days} \times \text{day\_duration}$$
Values equal to or exceeding the configured day duration (default `7h42m`) are converted to days (e.g. `1d` for `7h42m`).


> **Note:** Code Lens and Inlay Hints must be enabled in your Zed settings. See [Zed Settings](#zed-settings) below.

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

## Zed Settings

The inline project breakdown is surfaced via two complementary LSP features. Enable the one that suits your workflow in your Zed `settings.json` (`⌘⇧P` → **zed: open settings**).

### Code Lens

Displays the project breakdown as a clickable annotation **above** the first line of the file, in the gutter area.

```jsonc
// settings.json
{
  "code_lens": "on"   // "on" | "off"
}
```

> You can also toggle Code Lens per-language to keep it scoped to klog files only:
> ```jsonc
> {
>   "languages": {
>     "klog": {
>       "code_lens": "on"
>     }
>   }
> }
> ```

### Inlay Hints

Displays the project breakdown as **inline ghost text** directly inside the editor buffer at the start of the first line.

```jsonc
// settings.json
{
  "inlay_hints": {
    "enabled": true
  }
}
```

> Scope it to klog files only:
> ```jsonc
> {
>   "languages": {
>     "klog": {
>       "inlay_hints": {
>         "enabled": true
>       }
>     }
>   }
> }
> ```

### Recommended: enable both

```jsonc
// settings.json
{
  "languages": {
    "klog": {
      "code_lens": "on",
      "inlay_hints": {
        "enabled": true
      }
    }
  }
}
```

### Day Duration

By default, any duration equal to or exceeding 7 hours and 42 minutes (`7h42m`) is converted and formatted as a day (e.g. `1d` for `7h42m`). You can customize this threshold in your `settings.json` using the `day_duration` option under `initialization_options` for `klog-lsp`:

```jsonc
// settings.json
{
  "lsp": {
    "klog-lsp": {
      "initialization_options": {
        "day_duration": "8h"  // Supports standard klog duration strings (e.g. "8h", "7h30m", "1h30m", "450m") or integers (< 24 for hours, >= 24 for minutes)
      }
    }
  }
}
```

### Custom Klog Executable Path

By default, the LSP will search for a `klog` executable in your `$PATH`. If your `klog` binary is installed in a custom location, you can configure the exact path in your `settings.json` under `initialization_options` for `klog-lsp`:

```jsonc
// settings.json
{
  "lsp": {
    "klog-lsp": {
      "initialization_options": {
        "klog_path": "/path/to/custom/klog"
      }
    }
  }
}
```

### Document Formatting & Code Actions

This extension supports built-in formatting, diagnostics, and quick-fix code actions:
- **Format Document**: Runs `klog print` on your file to keep structures clean and aligned. To prevent indentation/syntax errors, all time entries are formatted with exactly **4 spaces of indentation** (e.g., `    8:15 - 8:30` instead of `     8:15 - 8:30` for single digit hours), and project tags/descriptions are aligned to consistent columns. You can trigger this manually (`⌘⇧I` or format document command) or enable format-on-save in Zed.
- **Diagnostics**: Real-time syntax errors and logical warnings from `klog json` are displayed inline as editor diagnostics.
- **Start/Stop Timer Code Actions**:
  - **Start open-ended time entry at [current_time]**: Offered when your cursor is inside a record. It appends a new open-ended time entry `    HH:MM - ?` at the current time.
  - **Stop active timer at [current_time] (line X)**: Offered when there are open-ended time entries (`?`) in your document. It replaces the selected `?` with the current time.

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
