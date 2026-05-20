use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::process::Command;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct HoverParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    position: Position,
}

#[derive(Deserialize, Debug)]
struct TextDocumentIdentifier {
    uri: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Position {
    line: u32,
    character: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Range {
    start: Position,
    end: Position,
}

#[derive(Deserialize, Debug)]
struct DidOpenTextDocumentParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentItem,
}

#[derive(Deserialize, Debug)]
struct TextDocumentItem {
    uri: String,
    text: String,
}

#[derive(Deserialize, Debug)]
struct DidChangeTextDocumentParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    #[serde(rename = "contentChanges")]
    content_changes: Vec<TextDocumentContentChangeEvent>,
}

#[derive(Deserialize, Debug)]
struct TextDocumentContentChangeEvent {
    text: String,
}

#[derive(Deserialize, Debug)]
struct DidCloseTextDocumentParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
}

#[derive(Deserialize, Debug)]
struct CodeLensParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
}

#[derive(Serialize, Debug)]
struct CodeLens {
    range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<CommandInfo>,
}

#[derive(Serialize, Debug)]
struct CommandInfo {
    title: String,
    command: String,
}

#[derive(Deserialize, Debug)]
struct InlayHintParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    #[allow(dead_code)]
    range: Range,
}

#[derive(Serialize, Debug)]
struct InlayHint {
    position: Position,
    label: String,
    kind: Option<u32>,
    #[serde(rename = "paddingLeft", skip_serializing_if = "Option::is_none")]
    padding_left: Option<bool>,
    #[serde(rename = "paddingRight", skip_serializing_if = "Option::is_none")]
    padding_right: Option<bool>,
}

fn log_msg(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/klog-lsp.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if trimmed.to_lowercase().starts_with("content-length:") {
            if let Some(parts) = trimmed.split(':').nth(1) {
                content_length = parts.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }
    if content_length == 0 {
        return Ok(None);
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

fn write_message<W: Write>(writer: &mut W, message: &str) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{}", message.len(), message)?;
    writer.flush()?;
    Ok(())
}

fn send_response<W: Write>(
    writer: &mut W,
    id: serde_json::Value,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
) {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result,
        error,
    };
    if let Ok(msg) = serde_json::to_string(&resp) {
        log_msg(&format!("Sending response: {}", msg));
        let _ = write_message(writer, &msg);
    }
}

fn is_date_line(line: &str) -> bool {
    let line = line.trim_start();
    if line.len() < 10 {
        return false;
    }
    let bytes = line.as_bytes();
    bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && (bytes[4] == b'-' || bytes[4] == b'/')
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && (bytes[7] == b'-' || bytes[7] == b'/')
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
}

fn find_date_for_line(content: &str, line_idx: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if line_idx >= lines.len() {
        return None;
    }
    for i in (0..=line_idx).rev() {
        let line = lines[i];
        if is_date_line(line) {
            if let Some(date) = line.trim_start().split_whitespace().next() {
                let clean_date = date.trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '/');
                if clean_date.len() >= 10 {
                    return Some(clean_date.to_string());
                }
            }
        }
    }
    None
}

fn find_tag_under_cursor(line: &str, char_idx: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if char_idx >= chars.len() {
        return None;
    }

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-' || chars[i] == '=')
            {
                i += 1;
            }
            let end = i;
            if char_idx >= start && char_idx < end {
                let tag_str: String = chars[start + 1..end].iter().collect();
                if !tag_str.is_empty() {
                    return Some(tag_str);
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn run_klog_command(args: &[&str], input: &str) -> Option<String> {
    log_msg(&format!("Running klog {:?}", args));
    let mut child = Command::new("klog")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    let output = child.wait_with_output().ok()?;
    if output.status.success() {
        let stdout_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stdout_str.is_empty() {
            return Some(stdout_str);
        }
    } else {
        let stderr_str = String::from_utf8_lossy(&output.stderr).trim().to_string();
        log_msg(&format!("klog command failed. stderr: {}", stderr_str));
    }
    None
}

fn run_klog_for_date(date: &str, content: &str) -> Option<String> {
    let total_args = vec!["total", "--date", date, "--diff", "--no-style"];
    let total_output = run_klog_command(&total_args, content)?;

    let mut total_val = None;
    let mut should_val = None;
    let mut diff_val = None;

    for line in total_output.lines() {
        if let Some(v) = line.strip_prefix("Total:") {
            total_val = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Should:") {
            should_val = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Diff:") {
            diff_val = Some(v.trim().to_string());
        }
    }

    total_val.as_ref()?;

    let mut table = format!("### Day Report: {}\n\n", date);
    table.push_str("| Metric | Value |\n");
    table.push_str("| :--- | :--- |\n");
    if let Some(v) = &total_val {
        table.push_str(&format!("| **Total** | **{}** |\n", v));
    }
    if let Some(v) = &should_val {
        table.push_str(&format!("| Should | {} |\n", v));
    }
    if let Some(v) = &diff_val {
        table.push_str(&format!("| Diff | {} |\n", v));
    }

    Some(table)
}

/// Returns a compact single-line day summary for use in Code Lenses and Inlay Hints.
/// Example: "Total: 6h  │  Should: 8h!  │  Diff: -2h"
fn get_day_summary_inline(date: &str, content: &str) -> Option<String> {
    let args = vec!["total", "--date", date, "--diff", "--no-style"];
    let output = run_klog_command(&args, content)?;

    let mut total_val = None;
    let mut should_val = None;
    let mut diff_val = None;

    for line in output.lines() {
        if let Some(v) = line.strip_prefix("Total:") {
            total_val = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Should:") {
            should_val = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Diff:") {
            diff_val = Some(v.trim().to_string());
        }
    }

    let total = total_val?;
    let mut parts = vec![format!("Total: {}", total)];
    if let Some(s) = should_val {
        parts.push(format!("Should: {}", s));
    }
    if let Some(d) = diff_val {
        parts.push(format!("Diff: {}", d));
    }
    Some(format!("{}\n\n", parts.join("  │  ")))
}

fn run_klog_for_tag(tag: &str, content: &str) -> Option<String> {
    let total_args = vec!["total", "--tag", tag, "--no-style"];
    let total_output = run_klog_command(&total_args, content);

    let report_args = vec!["report", "--tag", tag, "--no-style"];
    let report_output = run_klog_command(&report_args, content);

    if total_output.is_none() && report_output.is_none() {
        return None;
    }

    let mut table = format!("### Tag Report: #{}\n\n", tag);
    table.push_str("| Date | Time |\n");
    table.push_str("| :--- | :--- |\n");

    // Parse `klog report --no-style` output.
    // Lines look like (heavily padded with spaces):
    //   "                       Total"          <- header, no dot token
    //   "2026 May    Tue 19.       3h"          <- data row, year+month present
    //   "            Wed 20.       5h"          <- data row, year+month omitted (same)
    //   "                    ========           <- separator
    //   "                          8h"          <- grand total, no dot token
    // We track the running year+month and emit one table row per date line.
    if let Some(report) = report_output {
        let mut cur_year = String::new();
        let mut cur_month = String::new();

        for line in report.lines() {
            let trimmed = line.trim();
            // Skip empty, separator and warning lines
            if trimmed.is_empty()
                || trimmed.starts_with('=')
                || trimmed.starts_with('[')
                || trimmed == "Total"
            {
                continue;
            }

            let tokens: Vec<&str> = trimmed.split_whitespace().collect();
            if tokens.len() < 2 {
                continue;
            }

            // Find a day token: ends with '.' and is a short numeric string (e.g. "19.")
            let day_token = tokens
                .iter()
                .find(|t| t.ends_with('.') && t.len() <= 4 && t[..t.len()-1].chars().all(|c| c.is_ascii_digit()));

            let day = match day_token {
                Some(d) => d.trim_end_matches('.'),
                None => continue, // no day token → not a data row
            };

            // The last token is the time value
            let time = tokens.last().unwrap();
            // Guard: last token must look like a duration (contains digit)
            if !time.chars().any(|c| c.is_ascii_digit()) {
                continue;
            }

            // Update running year / month when tokens before the day include them
            // Year token: 4-digit number; month token: 3-letter alpha (May, Jun…)
            let day_pos = tokens.iter().position(|t| t.ends_with('.') && t.len() <= 4).unwrap_or(0);
            for t in &tokens[..day_pos] {
                if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
                    cur_year = t.to_string();
                } else if t.len() == 3 && t.chars().all(|c| c.is_ascii_alphabetic()) {
                    cur_month = t.to_string();
                }
            }

            let date_label = format!("{} {} {}", cur_year, cur_month, day);
            table.push_str(&format!("| {} | {} |\n", date_label.trim(), time));
        }
    }

    // Bold total row
    if let Some(total) = total_output {
        if let Some(line) = total.lines().find(|l| l.starts_with("Total:")) {
            let total_val = line.strip_prefix("Total:").unwrap().trim();
            table.push_str(&format!("| **Total** | **{}** |\n", total_val));
        }
    }

    Some(table)
}

fn get_project_breakdown(content: &str) -> Option<String> {
    get_project_aligned_text(content)
}

fn get_project_aligned_text(content: &str) -> Option<String> {
    let args = vec!["tags", "--values", "--no-style"];
    let output = run_klog_command(&args, content)?;

    let mut lines = output.lines().peekable();
    let mut project_values = Vec::new();
    let mut total_time = None;

    while let Some(line) = lines.next() {
        if line.starts_with("#project ") || line.starts_with("#project\t") || line == "#project" {
            if let Some(total) = line.split_whitespace().nth(1) {
                total_time = Some(total.to_string());
            }

            while let Some(next_line) = lines.peek() {
                if next_line.starts_with(' ') || next_line.starts_with('\t') {
                    let val_line = lines.next().unwrap().trim();
                    let parts: Vec<&str> = val_line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let val_name = parts[0];
                        let val_total = parts[1];
                        project_values.push((format!("#project={}", val_name), val_total.to_string()));
                    }
                } else {
                    break;
                }
            }
            break;
        }
    }

    if project_values.is_empty() {
        return None;
    }

    let mut max_width = "Project".len();
    for (name, _) in &project_values {
        if name.len() > max_width {
            max_width = name.len();
        }
    }
    
    let total_label = "Total";
    if total_label.len() > max_width {
        max_width = total_label.len();
    }

    let mut text_table = String::new();
    for (name, time) in &project_values {
        text_table.push_str(&format!("{:<width$} │ {}\n", name, time, width = max_width));
    }
    if let Some(total) = total_time {
        text_table.push_str(&format!("{:<width$} │ {}", total_label, total, width = max_width));
    }

    Some(text_table)
}

fn get_project_table_report(content: &str) -> Option<String> {
    let args = vec!["tags", "--values", "--no-style"];
    let output = run_klog_command(&args, content)?;

    let mut lines = output.lines().peekable();
    let mut project_values = Vec::new();
    let mut total_time = None;

    while let Some(line) = lines.next() {
        if line.starts_with("#project ") || line.starts_with("#project\t") || line == "#project" {
            if let Some(total) = line.split_whitespace().nth(1) {
                total_time = Some(total.to_string());
            }

            while let Some(next_line) = lines.peek() {
                if next_line.starts_with(' ') || next_line.starts_with('\t') {
                    let val_line = lines.next().unwrap().trim();
                    let parts: Vec<&str> = val_line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let val_name = parts[0];
                        let val_total = parts[1];
                        project_values.push((val_name.to_string(), val_total.to_string()));
                    }
                } else {
                    break;
                }
            }
            break;
        }
    }

    if project_values.is_empty() {
        return None;
    }

    let mut table = String::new();
    table.push_str("### Project Report\n\n");
    table.push_str("| Project | Total Time |\n");
    table.push_str("| :--- | :--- |\n");

    for (name, time) in project_values {
        table.push_str(&format!("| `#project={}` | {} |\n", name, time));
    }

    if let Some(total) = total_time {
        table.push_str(&format!("| **Total** | **{}** |\n", total));
    }

    Some(table)
}

fn main() {
    let _ = std::fs::remove_file("/tmp/klog-lsp.log");
    log_msg("klog-lsp starting...");

    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::stdout();

    let mut documents: HashMap<String, String> = HashMap::new();

    while let Ok(Some(msg)) = read_message(&mut reader) {
        log_msg(&format!("Received message: {}", msg));
        if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(&msg) {
            log_msg(&format!("Parsed request: {}", req.method));
            match req.method.as_str() {
                "initialize" => {
                    let result = serde_json::json!({
                        "capabilities": {
                            "textDocumentSync": 1, // Full sync
                            "hoverProvider": true,
                            "codeLensProvider": {
                                "resolveProvider": false
                            },
                            "inlayHintProvider": true,
                        }
                    });
                    send_response(
                        &mut writer,
                        req.id.clone(),
                        Some(result),
                        None,
                    );
                }
                "shutdown" => {
                    send_response(
                        &mut writer,
                        req.id.clone(),
                        Some(serde_json::json!(null)),
                        None,
                    );
                }
                "textDocument/hover" => {
                    log_msg("Handling textDocument/hover");
                    if let Some(params) = req.params {
                        if let Ok(hover_params) = serde_json::from_value::<HoverParams>(params) {
                            let uri = hover_params.text_document.uri;
                            let line_idx = hover_params.position.line as usize;
                            let char_idx = hover_params.position.character as usize;
                            log_msg(&format!("Hover request for uri: {}, line: {}, char: {}", uri, line_idx, char_idx));

                            let mut hover_content = None;
                            if let Some(content) = documents.get(&uri) {
                                let lines: Vec<&str> = content.lines().collect();
                                if line_idx < lines.len() {
                                    let current_line = lines[line_idx];

                                    // 1. Check if hovering a tag
                                    if let Some(tag) = find_tag_under_cursor(current_line, char_idx) {
                                        log_msg(&format!("Hovering tag: {}", tag));
                                        hover_content = run_klog_for_tag(&tag, content);
                                    }
                                    // 2. Check if hovering the title (date line)
                                    else if is_date_line(current_line) {
                                        if let Some(date) = find_date_for_line(content, line_idx) {
                                            log_msg(&format!("Hovering title/date: {}", date));
                                            let mut content_str = String::new();
                                            if let Some(day_report) = run_klog_for_date(&date, content) {
                                                content_str.push_str(&day_report);
                                                content_str.push_str("\n---\n");
                                            }
                                            if line_idx == 0 {
                                                if let Some(project_report) = get_project_table_report(content) {
                                                    content_str.push_str(&project_report);
                                                }
                                            }
                                            if !content_str.is_empty() {
                                                hover_content = Some(content_str);
                                            }
                                        }
                                    }
                                }
                            } else {
                                log_msg(&format!("Document not found in cache for URI: {}", uri));
                            }

                            log_msg(&format!("Hover content generated: {:?}", hover_content));
                            let result = if let Some(text) = hover_content {
                                serde_json::json!({
                                    "contents": {
                                        "kind": "markdown",
                                        "value": text,
                                    }
                                })
                            } else {
                                serde_json::Value::Null
                            };
                            send_response(
                                &mut writer,
                                req.id.clone(),
                                Some(result),
                                None,
                            );
                        } else {
                            log_msg("Failed to parse HoverParams");
                        }
                    }
                }
                "textDocument/codeLens" => {
                    log_msg("Handling textDocument/codeLens");
                    if let Some(params) = req.params {
                        if let Ok(lens_params) = serde_json::from_value::<CodeLensParams>(params) {
                            let uri = lens_params.text_document.uri;
                            let mut lenses = Vec::new();

                            if let Some(content) = documents.get(&uri) {
                                // Project breakdown at line 0
                                if let Some(breakdown) = get_project_breakdown(content) {
                                    lenses.push(CodeLens {
                                        range: Range {
                                            start: Position { line: 0, character: 0 },
                                            end: Position { line: 0, character: 0 },
                                        },
                                        command: Some(CommandInfo {
                                            title: breakdown,
                                            command: "".to_string(),
                                        }),
                                    });
                                }

                                // One compact day summary above each date line
                                for (idx, line) in content.lines().enumerate() {
                                    if is_date_line(line) {
                                        if let Some(date) = line.trim_start().split_whitespace().next() {
                                            let clean = date.trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '/');
                                            if clean.len() >= 10 {
                                                if let Some(summary) = get_day_summary_inline(clean, content) {
                                                    lenses.push(CodeLens {
                                                        range: Range {
                                                            start: Position { line: idx as u32, character: 0 },
                                                            end: Position { line: idx as u32, character: 0 },
                                                        },
                                                        command: Some(CommandInfo {
                                                            title: summary,
                                                            command: "".to_string(),
                                                        }),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            send_response(
                                &mut writer,
                                req.id.clone(),
                                Some(serde_json::json!(lenses)),
                                None,
                            );
                        } else {
                            log_msg("Failed to parse CodeLensParams");
                        }
                    }
                }
                "textDocument/inlayHint" => {
                    log_msg("Handling textDocument/inlayHint");
                    if let Some(params) = req.params {
                        if let Ok(hint_params) = serde_json::from_value::<InlayHintParams>(params) {
                            let uri = hint_params.text_document.uri;
                            let mut hints = Vec::new();

                            if let Some(content) = documents.get(&uri) {
                                // Project breakdown at line 0
                                if let Some(breakdown) = get_project_breakdown(content) {
                                    hints.push(InlayHint {
                                        position: Position { line: 0, character: 0 },
                                        label: breakdown,
                                        kind: Some(1),
                                        padding_left: Some(false),
                                        padding_right: Some(true),
                                    });
                                }

                                // One compact day summary at the end of each date line
                                for (idx, line) in content.lines().enumerate() {
                                    if is_date_line(line) {
                                        if let Some(date) = line.trim_start().split_whitespace().next() {
                                            let clean = date.trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '/');
                                            if clean.len() >= 10 {
                                                if let Some(summary) = get_day_summary_inline(clean, content) {
                                                    hints.push(InlayHint {
                                                        position: Position {
                                                            line: idx as u32,
                                                            character: line.len() as u32,
                                                        },
                                                        label: format!("  │  {}", summary),
                                                        kind: Some(1),
                                                        padding_left: Some(true),
                                                        padding_right: Some(false),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            send_response(
                                &mut writer,
                                req.id.clone(),
                                Some(serde_json::json!(hints)),
                                None,
                            );
                        } else {
                            log_msg("Failed to parse InlayHintParams");
                        }
                    }
                }
                _ => {
                    log_msg(&format!("Unhandled request method: {}", req.method));
                    send_response(
                        &mut writer,
                        req.id.clone(),
                        None,
                        Some(serde_json::json!({
                            "code": -32601,
                            "message": "Method not found"
                        })),
                    );
                }
            }
        } else if let Ok(notif) = serde_json::from_str::<JsonRpcNotification>(&msg) {
            log_msg(&format!("Parsed notification: {}", notif.method));
            match notif.method.as_str() {
                "exit" => {
                    log_msg("Exit notification received. Exiting.");
                    break;
                }
                "textDocument/didOpen" => {
                    if let Some(params) = notif.params {
                        if let Ok(open_params) =
                            serde_json::from_value::<DidOpenTextDocumentParams>(params)
                        {
                            log_msg(&format!("Opened document: {}", open_params.text_document.uri));
                            documents.insert(
                                open_params.text_document.uri,
                                open_params.text_document.text,
                            );
                        } else {
                            log_msg("Failed to parse DidOpenTextDocumentParams");
                        }
                    }
                }
                "textDocument/didChange" => {
                    if let Some(params) = notif.params {
                        if let Ok(change_params) =
                            serde_json::from_value::<DidChangeTextDocumentParams>(params)
                        {
                            log_msg(&format!("Changed document: {}", change_params.text_document.uri));
                            if let Some(change) = change_params.content_changes.first() {
                                documents.insert(
                                    change_params.text_document.uri,
                                    change.text.clone(),
                                );
                            }
                        } else {
                            log_msg("Failed to parse DidChangeTextDocumentParams");
                        }
                    }
                }
                "textDocument/didClose" => {
                    if let Some(params) = notif.params {
                        if let Ok(close_params) =
                            serde_json::from_value::<DidCloseTextDocumentParams>(params)
                        {
                            log_msg(&format!("Closed document: {}", close_params.text_document.uri));
                            documents.remove(&close_params.text_document.uri);
                        } else {
                            log_msg("Failed to parse DidCloseTextDocumentParams");
                        }
                    }
                }
                _ => {
                    log_msg(&format!("Unhandled notification method: {}", notif.method));
                }
            }
        } else {
            log_msg("Failed to parse request or notification JSON");
        }
    }
    log_msg("klog-lsp main loop ended.");
}
