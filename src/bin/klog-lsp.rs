use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::process::Command;

static DAY_DURATION_MINUTES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(462);
static KLOG_PATH: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

fn get_day_duration_minutes() -> i32 {
    let val = DAY_DURATION_MINUTES.load(std::sync::atomic::Ordering::Relaxed);
    if val <= 0 {
        462
    } else {
        val
    }
}

fn get_klog_path() -> String {
    if let Ok(guard) = KLOG_PATH.read() {
        if let Some(ref path) = *guard {
            return path.clone();
        }
    }
    "klog".to_string()
}

fn find_klog_path_recursively(value: &serde_json::Value) -> Option<String> {
    if let Some(obj) = value.as_object() {
        if let Some(val) = obj.get("klog_path") {
            if let Some(s) = val.as_str() {
                return Some(s.to_string());
            }
        }
        for (_, val) in obj {
            if let Some(res) = find_klog_path_recursively(val) {
                return Some(res);
            }
        }
    } else if let Some(arr) = value.as_array() {
        for val in arr {
            if let Some(res) = find_klog_path_recursively(val) {
                return Some(res);
            }
        }
    }
    None
}


/// Parses a duration string that can contain decimals (e.g. "7.7h", "8h", "7h30m", "450m") into total minutes.
/// Note that floats are not supported by the klog format itself, but are parsed here for configuration convenience.
/// Also supports bare numbers like "7.5" (hours) or "450" (minutes/hours depending on value).
fn parse_duration_to_minutes_flexible(s: &str) -> Option<i32> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let has_exclamation = trimmed.ends_with('!');
    let clean_s = if has_exclamation {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };
    let is_negative = clean_s.starts_with('-');
    let is_positive = clean_s.starts_with('+');
    let clean_s = if is_negative || is_positive {
        &clean_s[1..]
    } else {
        clean_s
    };

    let mut total_minutes = 0.0;
    let mut parsed = false;

    if clean_s.contains('h') && clean_s.contains('m') {
        let parts: Vec<&str> = clean_s.split('h').collect();
        if parts.len() == 2 {
            if let Ok(h) = parts[0].parse::<f64>() {
                let m_str = parts[1].trim_end_matches('m');
                if let Ok(m) = m_str.parse::<f64>() {
                    total_minutes = h * 60.0 + m;
                    parsed = true;
                }
            }
        }
    } else if clean_s.contains('h') {
        let h_str = clean_s.trim_end_matches('h');
        if let Ok(h) = h_str.parse::<f64>() {
            total_minutes = h * 60.0;
            parsed = true;
        }
    } else if clean_s.contains('m') {
        let m_str = clean_s.trim_end_matches('m');
        if let Ok(m) = m_str.parse::<f64>() {
            total_minutes = m;
            parsed = true;
        }
    } else {
        if let Ok(val) = clean_s.parse::<f64>() {
            if val.fract() == 0.0 {
                let val_i = val as i32;
                if val_i < 24 {
                    total_minutes = val * 60.0;
                } else {
                    total_minutes = val;
                }
            } else {
                total_minutes = val * 60.0;
            }
            parsed = true;
        }
    }

    if parsed {
        let mins = total_minutes.round() as i32;
        if is_negative {
            Some(-mins)
        } else {
            Some(mins)
        }
    } else {
        None
    }
}

/// Parses a json day_duration value (which could be number or string) into minutes.
fn parse_day_duration_setting(value: &serde_json::Value) -> Option<i32> {
    match value {
        serde_json::Value::Number(num) => {
            if let Some(f) = num.as_f64() {
                if f.fract() == 0.0 {
                    let val = f as i32;
                    if val < 24 {
                        Some(val * 60)
                    } else {
                        Some(val)
                    }
                } else {
                    Some((f * 60.0).round() as i32)
                }
            } else {
                None
            }
        }
        serde_json::Value::String(s) => parse_duration_to_minutes_flexible(s),
        _ => None,
    }
}

/// Recursively searches for the "day_duration" key in a JSON Value.
fn find_day_duration_recursively(value: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(obj) = value.as_object() {
        if let Some(val) = obj.get("day_duration") {
            return Some(val.clone());
        }
        for (_, val) in obj {
            if let Some(res) = find_day_duration_recursively(val) {
                return Some(res);
            }
        }
    } else if let Some(arr) = value.as_array() {
        for val in arr {
            if let Some(res) = find_day_duration_recursively(val) {
                return Some(res);
            }
        }
    }
    None
}

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

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct CodeActionParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    range: Range,
    context: CodeActionContext,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct CodeActionContext {
    diagnostics: Vec<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct FoldingRangeParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
struct FoldingRange {
    #[serde(rename = "startLine")]
    start_line: u32,
    #[serde(rename = "endLine")]
    end_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
}

#[derive(Serialize, Debug)]
struct CodeAction {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit: Option<WorkspaceEdit>,
    #[serde(rename = "isPreferred", skip_serializing_if = "Option::is_none")]
    is_preferred: Option<bool>,
}

#[derive(Serialize, Debug)]
struct WorkspaceEdit {
    #[serde(skip_serializing_if = "Option::is_none")]
    changes: Option<HashMap<String, Vec<TextEdit>>>,
}

#[derive(Serialize, Debug, Clone)]
struct TextEdit {
    range: Range,
    #[serde(rename = "newText")]
    new_text: String,
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

fn send_notification<W: Write>(
    writer: &mut W,
    method: &str,
    params: Option<serde_json::Value>,
) {
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });
    if let Ok(msg) = serde_json::to_string(&notif) {
        log_msg(&format!("Sending notification: {}", msg));
        let _ = write_message(writer, &msg);
    }
}


#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct KlogJsonOutput {
    records: Option<serde_json::Value>,
    warnings: Option<Vec<String>>,
    errors: Option<Vec<KlogJsonError>>,
}

#[derive(Deserialize, Debug)]
struct KlogJsonError {
    line: usize,
    column: usize,
    length: usize,
    title: String,
    details: String,
}

fn publish_diagnostics<W: Write>(uri: &str, content: &str, writer: &mut W) {
    let mut diagnostics = Vec::new();

    // Call klog json on content
    if let Some(json_str) = run_klog_command_raw(&["json"], content) {
        if let Ok(output) = serde_json::from_str::<KlogJsonOutput>(&json_str) {
            // Process errors
            if let Some(errors) = output.errors {
                for err in errors {
                    let line_0 = err.line.saturating_sub(1) as u32;
                    let col_0 = err.column.saturating_sub(1) as u32;
                    let length = err.length as u32;

                    diagnostics.push(serde_json::json!({
                        "range": {
                            "start": { "line": line_0, "character": col_0 },
                            "end": { "line": line_0, "character": col_0 + length }
                        },
                        "severity": 1, // Error
                        "code": "klog-syntax",
                        "source": "klog",
                        "message": format!("{}: {}", err.title, err.details)
                    }));
                }
            }

            // Process warnings
            if let Some(warnings) = output.warnings {
                for warn in warnings {
                    let parts: Vec<&str> = warn.splitn(2, ':').collect();
                    let mut matched_line = None;
                    if parts.len() == 2 {
                        let prefix = parts[0].trim();
                        if !prefix.is_empty() {
                            for (idx, line) in content.lines().enumerate() {
                                if line.contains(prefix) {
                                    matched_line = Some(idx);
                                    break;
                                }
                            }
                        }
                    }

                    let line_idx = matched_line.unwrap_or(0);
                    let line_len = content.lines().nth(line_idx).map(|l| l.len()).unwrap_or(0);

                    diagnostics.push(serde_json::json!({
                        "range": {
                            "start": { "line": line_idx as u32, "character": 0 },
                            "end": { "line": line_idx as u32, "character": line_len as u32 }
                        },
                        "severity": 2, // Warning
                        "code": "klog-warning",
                        "source": "klog",
                        "message": warn.clone()
                    }));
                }
            }
        }
    }

    let params = serde_json::json!({
        "uri": uri,
        "diagnostics": diagnostics
    });
    send_notification(writer, "textDocument/publishDiagnostics", Some(params));
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
    let klog_executable = get_klog_path();
    log_msg(&format!("Running {} {:?}", klog_executable, args));
    let mut child = Command::new(&klog_executable)
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

fn run_klog_command_raw(args: &[&str], input: &str) -> Option<String> {
    let klog_executable = get_klog_path();
    log_msg(&format!("Running raw {} {:?}", klog_executable, args));
    let mut child = Command::new(&klog_executable)
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
        let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
        return Some(stdout_str);
    } else {
        let stderr_str = String::from_utf8_lossy(&output.stderr).trim().to_string();
        log_msg(&format!("klog command failed. stderr: {}", stderr_str));
    }
    None
}


fn format_days(days: f64) -> String {
    let s = format!("{:.2}", days);
    if s.ends_with(".00") {
        s[..s.len() - 3].to_string()
    } else if s.ends_with('0') && s.contains('.') {
        s[..s.len() - 1].to_string()
    } else {
        s
    }
}

fn convert_duration_string(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return s.to_string();
    }

    let has_exclamation = trimmed.ends_with('!');
    let clean_s = if has_exclamation {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };

    let is_negative = clean_s.starts_with('-');
    let is_positive = clean_s.starts_with('+');
    let clean_s = if is_negative || is_positive {
        &clean_s[1..]
    } else {
        clean_s
    };

    let mut hours = 0;
    let mut minutes = 0;
    let mut parsed = false;

    if clean_s.contains('h') && clean_s.contains('m') {
        let parts: Vec<&str> = clean_s.split('h').collect();
        if parts.len() == 2 {
            if let Ok(h) = parts[0].parse::<i32>() {
                let m_str = parts[1].trim_end_matches('m');
                if let Ok(m) = m_str.parse::<i32>() {
                    hours = h;
                    minutes = m;
                    parsed = true;
                }
            }
        }
    } else if clean_s.contains('h') {
        let h_str = clean_s.trim_end_matches('h');
        if let Ok(h) = h_str.parse::<i32>() {
            hours = h;
            parsed = true;
        }
    } else if clean_s.contains('m') {
        let m_str = clean_s.trim_end_matches('m');
        if let Ok(m) = m_str.parse::<i32>() {
            minutes = m;
            parsed = true;
        }
    }

    if !parsed {
        return s.to_string();
    }

    let total_minutes = hours * 60 + minutes;
    let day_duration = get_day_duration_minutes();
    if total_minutes >= day_duration {
        let days = total_minutes as f64 / day_duration as f64;
        let mut formatted = format_days(days);
        if is_negative {
            formatted = format!("-{}", formatted);
        } else if is_positive {
            formatted = format!("+{}", formatted);
        }
        formatted.push('d');
        if has_exclamation {
            formatted.push('!');
        }
        formatted
    } else {
        s.to_string()
    }
}

fn get_current_date() -> Option<String> {
    Some(chrono::Local::now().format("%Y-%m-%d").to_string())
}

fn get_current_time() -> Option<String> {
    Some(chrono::Local::now().format("%H:%M").to_string())
}

fn extract_tags(content: &str) -> Vec<String> {
    let mut tags = std::collections::HashSet::new();
    for line in content.lines() {
        for word in line.split_whitespace() {
            if word.starts_with('#') {
                let tag = word.trim_end_matches(|c: char| c.is_ascii_punctuation() && c != '=' && c != '-' && c != '_');
                if tag.len() > 1 {
                    tags.insert(tag.to_string());
                    if let Some(pos) = tag.find('=') {
                        tags.insert(tag[..=pos].to_string());
                    }
                }
            }
        }
    }
    let mut tag_list: Vec<String> = tags.into_iter().collect();
    tag_list.sort();
    tag_list
}

fn find_open_range_question_mark(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let c = trimmed.chars().next()?;
    if !c.is_ascii_digit() && c != '<' && c != '>' {
        return None;
    }
    if let Some(hyphen_idx) = line.find('-') {
        let after_hyphen = &line[hyphen_idx + 1..];
        if let Some(q_offset) = after_hyphen.find('?') {
            let between = &after_hyphen[..q_offset];
            if between.chars().all(|c| c.is_whitespace()) {
                return Some(hyphen_idx + 1 + q_offset);
            }
        }
    }
    None
}

fn get_code_actions(
    uri: &str,
    content: &str,
    range: Range,
) -> Vec<CodeAction> {
    let mut actions = Vec::new();
    let current_time = match get_current_time() {
        Some(t) => t,
        None => return actions,
    };

    let lines: Vec<&str> = content.lines().collect();

    // 1. "Start open-ended time entry at [current_time]"
    let cursor_line = range.start.line as usize;
    if cursor_line < lines.len() {
        if find_date_for_line(content, cursor_line).is_some() {
            let line_content = lines[cursor_line];
            let end_char = line_content.len() as u32;
            
            let mut changes = HashMap::new();
            changes.insert(
                uri.to_string(),
                vec![TextEdit {
                    range: Range {
                        start: Position {
                            line: cursor_line as u32,
                            character: end_char,
                        },
                        end: Position {
                            line: cursor_line as u32,
                            character: end_char,
                        },
                    },
                    new_text: format!("\n    {} - ?", current_time),
                }],
            );

            actions.push(CodeAction {
                title: format!("Start open-ended time entry at {}", current_time),
                kind: Some("quickfix".to_string()),
                edit: Some(WorkspaceEdit { changes: Some(changes) }),
                is_preferred: Some(true),
            });
        }
    }

    // 2. "Stop active timer at [current_time]"
    for (line_idx, line) in lines.iter().enumerate() {
        if line.contains('?') {
            if find_date_for_line(content, line_idx).is_some() {
                if let Some(char_idx) = find_open_range_question_mark(line) {
                    let mut changes = HashMap::new();
                    changes.insert(
                        uri.to_string(),
                        vec![TextEdit {
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: char_idx as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: (char_idx + 1) as u32,
                                },
                            },
                            new_text: current_time.clone(),
                        }],
                    );

                    actions.push(CodeAction {
                        title: format!(
                            "Stop active timer at {} (line {})",
                            current_time,
                            line_idx + 1
                        ),
                        kind: Some("quickfix".to_string()),
                        edit: Some(WorkspaceEdit { changes: Some(changes) }),
                        is_preferred: Some(true),
                    });
                }
            }
        }
    }

    actions
}

fn get_completions(date_str: &str, time_str: &str, content: Option<&str>) -> serde_json::Value {
    let hour = if time_str.len() >= 2 { &time_str[..2] } else { "00" };
    let mins = get_day_duration_minutes();
    let duration_str = format_minutes_to_duration(mins);

    let date_with_duration = format!("{} ({}!)", date_str, duration_str);
    let date_snippet_prefilled = format!("${{1:{}}} (${{2:{}!}})", date_str, duration_str);
    let date_snippet_not_prefilled = format!("${{1:YYYY-MM-DD}} (${{2:{}!}})", duration_str);

    let mut items = serde_json::json!([
        {
            "label": "today",
            "insertText": date_snippet_prefilled,
            "insertTextFormat": 2,
            "kind": 15,
            "detail": format!("Inserts today's date with configured day duration ({})", date_with_duration)
        },
        {
            "label": "date",
            "insertText": date_snippet_not_prefilled,
            "insertTextFormat": 2,
            "kind": 15,
            "detail": format!("Inserts a date record template with configured day duration (YYYY-MM-DD ({}!))", duration_str)
        },
        {
            "label": "20",
            "insertText": date_snippet_prefilled,
            "insertTextFormat": 2,
            "kind": 15,
            "detail": format!("Inserts today's date with configured day duration ({})", date_with_duration)
        },
        {
            "label": date_str,
            "insertText": date_snippet_prefilled,
            "insertTextFormat": 2,
            "kind": 15,
            "detail": format!("Inserts today's date with configured day duration ({})", date_with_duration)
        },
        {
            "label": "record",
            "insertText": format!("${{1:{}}} (${{2:{}!}})\n${{3:Summary}}\n    $0", date_str, duration_str),
            "insertTextFormat": 2,
            "kind": 15,
            "detail": "Creates a new record block with today's date and configured day duration"
        },
        {
            "label": "time",
            "insertText": time_str,
            "kind": 15,
            "detail": format!("Inserts current time ({})", time_str)
        },
        {
            "label": "ts",
            "insertText": format!("{}:${{1:00}} - {}:${{2:00}} $0", hour, hour),
            "insertTextFormat": 2,
            "kind": 15,
            "detail": "Inserts a timespan starting at the current hour"
        },
        {
            "label": "timespan",
            "insertText": format!("{}:${{1:00}} - {}:${{2:00}} $0", hour, hour),
            "insertTextFormat": 2,
            "kind": 15,
            "detail": "Inserts a timespan starting at the current hour"
        },
        {
            "label": "tsoe",
            "insertText": format!("{} - ? $0", time_str),
            "insertTextFormat": 2,
            "kind": 15,
            "detail": "Inserts an open-ended timespan starting at the current time"
        },
        {
            "label": "timespan-open-ended",
            "insertText": format!("{} - ? $0", time_str),
            "insertTextFormat": 2,
            "kind": 15,
            "detail": "Inserts an open-ended timespan starting at the current time"
        }
    ]);

    if let Some(text) = content {
        if let Some(arr) = items.as_array_mut() {
            for tag in extract_tags(text) {
                arr.push(serde_json::json!({
                    "label": tag,
                    "insertText": tag,
                    "kind": 12,
                    "detail": "Tag from document"
                }));
            }
        }
    }

    items
}

/// Sakamoto's algorithm: returns 0 for Sunday, 1 for Monday, ..., 6 for Saturday.
fn day_of_week(y: i32, m: i32, d: i32) -> i32 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = y;
    if m < 3 {
        y -= 1;
    }
    (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d) % 7
}

/// Returns true if the day is a weekday (Monday through Friday).
fn is_weekday(y: i32, m: i32, d: i32) -> bool {
    let dow = day_of_week(y, m, d);
    dow != 0 && dow != 6
}

/// Returns the number of working days (weekdays) in the given month of the given year.
fn working_days_in_month(year: i32, month: i32) -> i32 {
    let mut working_days = 0;
    let days = days_in_month(year, month);
    for d in 1..=days {
        if is_weekday(year, month, d) {
            working_days += 1;
        }
    }
    working_days
}

/// Returns the number of days in the given month of the given year, taking leap years into account.
fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Scans the entire file content to identify the latest record date.
/// Returns a tuple of (Year, Month, Day) if any valid dates are found.
fn find_latest_date_in_content(content: &str) -> Option<(i32, i32, i32)> {
    let mut latest_date: Option<(i32, i32, i32)> = None;
    for line in content.lines() {
        if is_date_line(line) {
            if let Some(date_part) = line.trim_start().split_whitespace().next() {
                let clean = date_part.trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '/');
                let parts: Vec<&str> = if clean.contains('-') {
                    clean.split('-').collect()
                } else {
                    clean.split('/').collect()
                };
                if parts.len() == 3 {
                    if let (Ok(y), Ok(m), Ok(d)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>(), parts[2].parse::<i32>()) {
                        let cur = (y, m, d);
                        if let Some(prev) = latest_date {
                            if cur > prev {
                                latest_date = Some(cur);
                            }
                        } else {
                            latest_date = Some(cur);
                        }
                    }
                }
            }
        }
    }
    latest_date
}

/// Parses a duration string (e.g. "1h30m", "45m", "+2h", "-30m", "8h!") into total minutes.
fn parse_duration_to_minutes(s: &str) -> Option<i32> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    let has_exclamation = trimmed.ends_with('!');
    let clean_s = if has_exclamation {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };

    let is_negative = clean_s.starts_with('-');
    let is_positive = clean_s.starts_with('+');
    let clean_s = if is_negative || is_positive {
        &clean_s[1..]
    } else {
        clean_s
    };

    let mut hours = 0;
    let mut minutes = 0;
    let mut parsed = false;

    if clean_s.contains('h') && clean_s.contains('m') {
        let parts: Vec<&str> = clean_s.split('h').collect();
        if parts.len() == 2 {
            if let Ok(h) = parts[0].parse::<i32>() {
                let m_str = parts[1].trim_end_matches('m');
                if let Ok(m) = m_str.parse::<i32>() {
                    hours = h;
                    minutes = m;
                    parsed = true;
                }
            }
        }
    } else if clean_s.contains('h') {
        let h_str = clean_s.trim_end_matches('h');
        if let Ok(h) = h_str.parse::<i32>() {
            hours = h;
            parsed = true;
        }
    } else if clean_s.contains('m') {
        let m_str = clean_s.trim_end_matches('m');
        if let Ok(m) = m_str.parse::<i32>() {
            minutes = m;
            parsed = true;
        }
    }

    if parsed {
        let total = hours * 60 + minutes;
        if is_negative {
            Some(-total)
        } else {
            Some(total)
        }
    } else {
        None
    }
}

/// Converts total minutes back into a standard klog duration string (e.g. "1h30m", "45m", "-30m").
fn format_minutes_to_duration(total_mins: i32) -> String {
    if total_mins == 0 {
        return "0m".to_string();
    }
    let is_negative = total_mins < 0;
    let total_mins = total_mins.abs();
    let hours = total_mins / 60;
    let minutes = total_mins % 60;

    let mut s = String::new();
    if is_negative {
        s.push('-');
    }
    if hours > 0 && minutes > 0 {
        s.push_str(&format!("{}h{}m", hours, minutes));
    } else if hours > 0 {
        s.push_str(&format!("{}h", hours));
    } else {
        s.push_str(&format!("{}m", minutes));
    }
    s
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
        table.push_str(&format!("| **Total** | **{}** |\n", convert_duration_string(v)));
    }
    if let Some(v) = &should_val {
        table.push_str(&format!("| Should | {} |\n", convert_duration_string(v)));
    }
    if let Some(v) = &diff_val {
        table.push_str(&format!("| Diff | {} |\n", convert_duration_string(v)));
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
    let mut parts = vec![format!("Total: {}", convert_duration_string(&total))];
    if let Some(s) = should_val {
        parts.push(format!("Should: {}", convert_duration_string(&s)));
    }
    if let Some(d) = diff_val {
        parts.push(format!("Diff: {}", convert_duration_string(&d)));
    }
    Some(format!("{}", parts.join("  │  ")))
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
            let time_converted = convert_duration_string(time);
            table.push_str(&format!("| {} | {} |\n", date_label.trim(), time_converted));
        }
    }

    // Bold total row
    if let Some(total) = total_output {
        if let Some(line) = total.lines().find(|l| l.starts_with("Total:")) {
            let total_val = line.strip_prefix("Total:").unwrap().trim();
            table.push_str(&format!("| **Total** | **{}** |\n", convert_duration_string(total_val)));
        }
    }

    Some(table)
}

fn get_project_breakdown(content: &str) -> Option<String> {
    get_project_aligned_text(content)
}

fn should_estimate_project(name: &str) -> bool {
    let clean_name = name.strip_prefix("#project=").unwrap_or(name);
    !matches!(clean_name, "ABSCP" | "RTTE" | "RTTS" | "ABSConv" | "ABSMal")
}

fn get_project_aligned_text(content: &str) -> Option<String> {
    let latest_date = find_latest_date_in_content(content);
    #[allow(unused_assignments)]
    let mut period_str = String::new();
    
    let (args, days_info) = if let Some((year, month, day)) = latest_date {
        period_str = format!("{:04}-{:02}", year, month);
        (vec!["tags", "--values", "--period", &period_str, "--no-style"], Some((year, month, day)))
    } else {
        (vec!["tags", "--values", "--no-style"], None)
    };

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

    let ratio = if let Some((year, month, _day)) = days_info {
        if let Some(ref total_str) = total_time {
            if let Some(total_mins) = parse_duration_to_minutes(total_str) {
                if total_mins > 0 {
                    let working_days = working_days_in_month(year, month);
                    let day_duration = get_day_duration_minutes();
                    Some((working_days * day_duration) as f64 / total_mins as f64)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let header_project = "Project";
    let header_total = "Total";
    let header_est = "Est. End";

    let mut max_project_width = header_project.len();
    let mut max_total_width = header_total.len();
    let mut max_est_width = header_est.len();

    let total_mins = total_time.as_ref().and_then(|t| parse_duration_to_minutes(t));

    let mut raw_rows = Vec::new();
    let mut max_raw_total_width = 0;
    let mut max_raw_est_width = 0;

    for (name, raw_time) in &project_values {
        let time_raw = convert_duration_string(raw_time);
        let est_raw = if let Some(r) = ratio {
            if should_estimate_project(name) {
                if let Some(mins) = parse_duration_to_minutes(raw_time) {
                    let est_mins = ((mins as f64) * r).round() as i32;
                    convert_duration_string(&format_minutes_to_duration(est_mins))
                } else {
                    "-".to_string()
                }
            } else {
                time_raw.clone()
            }
        } else {
            "-".to_string()
        };

        max_raw_total_width = max_raw_total_width.max(time_raw.len());
        if est_raw != "-" {
            max_raw_est_width = max_raw_est_width.max(est_raw.len());
        }

        raw_rows.push((name.clone(), time_raw, est_raw));
    }

    let total_time_raw = if let Some(ref t) = total_time {
        convert_duration_string(t)
    } else {
        "-".to_string()
    };

    let total_est_raw = if let Some((year, month, _)) = latest_date {
        let working_days = working_days_in_month(year, month);
        let day_duration = get_day_duration_minutes();
        let est_mins = working_days * day_duration;
        convert_duration_string(&format_minutes_to_duration(est_mins))
    } else {
        "-".to_string()
    };

    max_raw_total_width = max_raw_total_width.max(total_time_raw.len());
    if total_est_raw != "-" {
        max_raw_est_width = max_raw_est_width.max(total_est_raw.len());
    }

    let mut rows = Vec::new();
    for (name, time_raw, est_raw) in raw_rows {
        let mut time_formatted = time_raw.clone();
        let mut est_formatted = est_raw.clone();

        if let Some(tot_m) = total_mins {
            if tot_m > 0 {
                if let Some((_, original_raw_time)) = project_values.iter().find(|(n, _)| format!("#project={}", n) == name || n == &name) {
                    if let Some(mins) = parse_duration_to_minutes(original_raw_time) {
                        let pct = (mins as f64 / tot_m as f64) * 100.0;
                        time_formatted = format!("{:<width$} ({:.1}%)", time_raw, pct, width = max_raw_total_width);
                        if est_raw != "-" {
                            est_formatted = format!("{:<width$} ({:.1}%)", est_raw, pct, width = max_raw_est_width);
                        }
                    }
                }
            }
        }

        max_project_width = max_project_width.max(name.len());
        max_total_width = max_total_width.max(time_formatted.len());
        max_est_width = max_est_width.max(est_formatted.len());

        rows.push((name.clone(), time_formatted, est_formatted));
    }

    let total_label = "Total";
    let total_time_formatted = if total_time.is_some() && total_mins.is_some() {
        format!("{:<width$} (100.0%)", total_time_raw, width = max_raw_total_width)
    } else {
        total_time_raw
    };

    let total_est_formatted = if latest_date.is_some() && total_mins.is_some() && total_est_raw != "-" {
        format!("{:<width$} (100.0%)", total_est_raw, width = max_raw_est_width)
    } else {
        total_est_raw
    };

    max_project_width = max_project_width.max(total_label.len());
    max_total_width = max_total_width.max(total_time_formatted.len());
    max_est_width = max_est_width.max(total_est_formatted.len());

    let mut text_table = String::new();
    
    // Header
    text_table.push_str(&format!(
        "{:<col1_w$} │ {:<col2_w$} │ {:<col3_w$}\n",
        header_project, header_total, header_est,
        col1_w = max_project_width, col2_w = max_total_width, col3_w = max_est_width
    ));

    // Separator
    let sep_col1 = "─".repeat(max_project_width + 1);
    let sep_col2 = "─".repeat(max_total_width + 2);
    let sep_col3 = "─".repeat(max_est_width + 1);
    text_table.push_str(&format!("{}┼{}┼{}\n", sep_col1, sep_col2, sep_col3));

    // Rows
    for (name, total, est) in rows {
        text_table.push_str(&format!(
            "{:<col1_w$} │ {:<col2_w$} │ {:<col3_w$}\n",
            name, total, est,
            col1_w = max_project_width, col2_w = max_total_width, col3_w = max_est_width
        ));
    }

    // Total row
    if total_time.is_some() {
        text_table.push_str(&format!("{}┼{}┼{}\n", sep_col1, sep_col2, sep_col3));
        text_table.push_str(&format!(
            "{:<col1_w$} │ {:<col2_w$} │ {:<col3_w$}",
            total_label, total_time_formatted, total_est_formatted,
            col1_w = max_project_width, col2_w = max_total_width, col3_w = max_est_width
        ));
    }

    Some(text_table)
}

fn get_project_table_report(content: &str) -> Option<String> {
    let latest_date = find_latest_date_in_content(content);
    #[allow(unused_assignments)]
    let mut period_str = String::new();
    
    let (args, days_info) = if let Some((year, month, day)) = latest_date {
        period_str = format!("{:04}-{:02}", year, month);
        (vec!["tags", "--values", "--period", &period_str, "--no-style"], Some((year, month, day)))
    } else {
        (vec!["tags", "--values", "--no-style"], None)
    };

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

    let ratio = if let Some((year, month, _day)) = days_info {
        if let Some(ref total_str) = total_time {
            if let Some(total_mins) = parse_duration_to_minutes(total_str) {
                if total_mins > 0 {
                    let working_days = working_days_in_month(year, month);
                    let day_duration = get_day_duration_minutes();
                    Some((working_days * day_duration) as f64 / total_mins as f64)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let total_mins = total_time.as_ref().and_then(|t| parse_duration_to_minutes(t));

    let mut raw_rows = Vec::new();
    let mut max_raw_total_width = 0;
    let mut max_raw_est_width = 0;

    for (name, raw_time) in &project_values {
        let time_raw = convert_duration_string(raw_time);
        let est_raw = if let Some(r) = ratio {
            if should_estimate_project(name) {
                if let Some(mins) = parse_duration_to_minutes(raw_time) {
                    let est_mins = ((mins as f64) * r).round() as i32;
                    convert_duration_string(&format_minutes_to_duration(est_mins))
                } else {
                    "-".to_string()
                }
            } else {
                time_raw.clone()
            }
        } else {
            "-".to_string()
        };

        max_raw_total_width = max_raw_total_width.max(time_raw.len());
        if est_raw != "-" {
            max_raw_est_width = max_raw_est_width.max(est_raw.len());
        }

        raw_rows.push((name.clone(), time_raw, est_raw));
    }

    let total_time_raw = if let Some(ref t) = total_time {
        convert_duration_string(t)
    } else {
        "-".to_string()
    };

    let total_est_raw = if let Some((year, month, _)) = latest_date {
        let working_days = working_days_in_month(year, month);
        let day_duration = get_day_duration_minutes();
        let est_mins = working_days * day_duration;
        convert_duration_string(&format_minutes_to_duration(est_mins))
    } else {
        "-".to_string()
    };

    max_raw_total_width = max_raw_total_width.max(total_time_raw.len());
    if total_est_raw != "-" {
        max_raw_est_width = max_raw_est_width.max(total_est_raw.len());
    }

    let mut table = String::new();
    table.push_str("### Project Report\n\n");
    table.push_str("| Project | Total Time | Est. End |\n");
    table.push_str("| :--- | :--- | :--- |\n");

    for (name, time_raw, est_raw) in raw_rows {
        let mut time_formatted = time_raw.clone();
        let mut est_formatted = est_raw.clone();

        if let Some(tot_m) = total_mins {
            if tot_m > 0 {
                if let Some((_, original_raw_time)) = project_values.iter().find(|(n, _)| format!("#project={}", n) == name || n == &name) {
                    if let Some(mins) = parse_duration_to_minutes(original_raw_time) {
                        let pct = (mins as f64 / tot_m as f64) * 100.0;
                        time_formatted = format!("{:<width$} ({:.1}%)", time_raw, pct, width = max_raw_total_width);
                        if est_raw != "-" {
                            est_formatted = format!("{:<width$} ({:.1}%)", est_raw, pct, width = max_raw_est_width);
                        }
                    }
                }
            }
        }

        table.push_str(&format!("| `#project={}` | {} | {} |\n", name, time_formatted, est_formatted));
    }

    if total_time.is_some() {
        let total_formatted = if total_mins.is_some() {
            format!("{:<width$} (100.0%)", total_time_raw, width = max_raw_total_width)
        } else {
            total_time_raw
        };

        let total_est_formatted = if latest_date.is_some() && total_mins.is_some() && total_est_raw != "-" {
            format!("{:<width$} (100.0%)", total_est_raw, width = max_raw_est_width)
        } else {
            total_est_raw
        };

        table.push_str(&format!("| **Total** | **{}** | **{}** |\n", total_formatted, total_est_formatted));
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
                    if let Some(ref params) = req.params {
                        if let Some(day_duration_val) = find_day_duration_recursively(params) {
                            if let Some(mins) = parse_day_duration_setting(&day_duration_val) {
                                log_msg(&format!("Setting DAY_DURATION_MINUTES (initialize) to {}", mins));
                                DAY_DURATION_MINUTES.store(mins, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        if let Some(path_val) = find_klog_path_recursively(params) {
                            log_msg(&format!("Setting KLOG_PATH (initialize) to {}", path_val));
                            if let Ok(mut guard) = KLOG_PATH.write() {
                                *guard = Some(path_val);
                            }
                        }
                    }
                    let result = serde_json::json!({
                        "capabilities": {
                            "textDocumentSync": 1, // Full sync
                            "hoverProvider": true,
                            "codeLensProvider": {
                                "resolveProvider": false
                            },
                            "inlayHintProvider": true,
                            "completionProvider": {
                                "resolveProvider": false
                            },
                            "documentFormattingProvider": true,
                            "codeActionProvider": true,
                            "foldingRangeProvider": true
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
                                // Project breakdown at line 0 with 1 trailing blank line
                                if let Some(breakdown) = get_project_breakdown(content) {
                                    lenses.push(CodeLens {
                                        range: Range {
                                            start: Position { line: 0, character: 0 },
                                            end: Position { line: 0, character: 0 },
                                        },
                                        command: Some(CommandInfo {
                                            title: format!("{}\n", breakdown),
                                            command: "".to_string(),
                                        }),
                                    });
                                }

                                // One compact day summary below each date line
                                let total_lines = content.lines().count();
                                for (idx, line) in content.lines().enumerate() {
                                    if is_date_line(line) {
                                        if let Some(date) = line.trim_start().split_whitespace().next() {
                                            let clean = date.trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '/');
                                            if clean.len() >= 10 {
                                                if let Some(summary) = get_day_summary_inline(clean, content) {
                                                    let target_line = if idx + 1 < total_lines { idx + 1 } else { idx };
                                                    lenses.push(CodeLens {
                                                        range: Range {
                                                            start: Position { line: target_line as u32, character: 0 },
                                                            end: Position { line: target_line as u32, character: 0 },
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
                                // Project breakdown at line 0 with 1 trailing blank line
                                if let Some(breakdown) = get_project_breakdown(content) {
                                    hints.push(InlayHint {
                                        position: Position { line: 0, character: 0 },
                                        label: format!("{}\n", breakdown),
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
                "textDocument/completion" => {
                    log_msg("Handling textDocument/completion");
                    
                    let mut doc_content: Option<String> = None;
                    if let Some(ref params) = req.params {
                        if let Some(uri_val) = params.get("textDocument").and_then(|td| td.get("uri")) {
                            if let Some(uri_str) = uri_val.as_str() {
                                if let Some(content) = documents.get(uri_str) {
                                    doc_content = Some(content.clone());
                                }
                            }
                        }
                    }

                    let date_str = get_current_date().unwrap_or_else(|| "2026-05-21".to_string());
                    let time_str = get_current_time().unwrap_or_else(|| "09:00".to_string());

                    let completions = get_completions(&date_str, &time_str, doc_content.as_deref());

                    send_response(
                        &mut writer,
                        req.id.clone(),
                        Some(completions),
                        None,
                    );
                }
                "textDocument/formatting" => {
                    log_msg("Handling textDocument/formatting");
                    let mut text_edits = serde_json::json!(null);

                    if let Some(ref params) = req.params {
                        if let Some(uri_val) = params.get("textDocument").and_then(|td| td.get("uri")) {
                            if let Some(uri_str) = uri_val.as_str() {
                                if let Some(content) = documents.get(uri_str) {
                                    // Run klog print --no-style --no-warn to format the document
                                    if let Some(formatted) = run_klog_command_raw(&["print", "--no-style", "--no-warn"], content) {
                                        let aligned = align_klog_content(&formatted);
                                        let mut final_text = aligned.trim_end().to_string();
                                        final_text.push('\n');

                                        let split_lines: Vec<&str> = content.split('\n').collect();
                                        let end_line = split_lines.len().saturating_sub(1);
                                        let end_char = split_lines.last().map(|l| l.len()).unwrap_or(0);

                                        text_edits = serde_json::json!([
                                            {
                                                "range": {
                                                    "start": { "line": 0, "character": 0 },
                                                    "end": { "line": end_line, "character": end_char }
                                                },
                                                "newText": final_text
                                            }
                                        ]);
                                    }
                                }
                            }
                        }
                    }

                    send_response(
                        &mut writer,
                        req.id.clone(),
                        Some(text_edits),
                        None,
                    );
                }
                "textDocument/foldingRange" => {
                    log_msg("Handling textDocument/foldingRange");
                    let mut folding_ranges = serde_json::json!([]);

                    if let Some(ref params) = req.params {
                        if let Ok(folding_params) =
                            serde_json::from_value::<FoldingRangeParams>(params.clone())
                        {
                            let uri_str = &folding_params.text_document.uri;
                            if let Some(content) = documents.get(uri_str) {
                                let ranges = get_folding_ranges(content);
                                folding_ranges = serde_json::to_value(ranges).unwrap_or(serde_json::json!([]));
                            }
                        } else {
                            log_msg("Failed to parse FoldingRangeParams");
                        }
                    }

                    send_response(
                        &mut writer,
                        req.id.clone(),
                        Some(folding_ranges),
                        None,
                    );
                }
                "textDocument/codeAction" => {
                    log_msg("Handling textDocument/codeAction");
                    let mut actions = serde_json::json!([]);

                    if let Some(ref params) = req.params {
                        if let Ok(code_action_params) =
                            serde_json::from_value::<CodeActionParams>(params.clone())
                        {
                            let uri_str = &code_action_params.text_document.uri;
                            if let Some(content) = documents.get(uri_str) {
                                let actions_list = get_code_actions(
                                    uri_str,
                                    content,
                                    code_action_params.range,
                                );
                                actions = serde_json::to_value(actions_list).unwrap_or(serde_json::json!([]));
                            }
                        } else {
                            log_msg("Failed to parse CodeActionParams");
                        }
                    }

                    send_response(
                        &mut writer,
                        req.id.clone(),
                        Some(actions),
                        None,
                    );
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
                "workspace/didChangeConfiguration" => {
                    if let Some(ref params) = notif.params {
                        if let Some(day_duration_val) = find_day_duration_recursively(params) {
                            if let Some(mins) = parse_day_duration_setting(&day_duration_val) {
                                log_msg(&format!("Setting DAY_DURATION_MINUTES (didChange) to {}", mins));
                                DAY_DURATION_MINUTES.store(mins, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        if let Some(path_val) = find_klog_path_recursively(params) {
                            log_msg(&format!("Setting KLOG_PATH (didChange) to {}", path_val));
                            if let Ok(mut guard) = KLOG_PATH.write() {
                                *guard = Some(path_val);
                            }
                        }
                        // Re-validate all documents
                        for (uri, content) in &documents {
                            publish_diagnostics(uri, content, &mut writer);
                        }
                    }
                }
                "textDocument/didOpen" => {
                    if let Some(params) = notif.params {
                        if let Ok(open_params) =
                            serde_json::from_value::<DidOpenTextDocumentParams>(params)
                        {
                            log_msg(&format!("Opened document: {}", open_params.text_document.uri));
                            documents.insert(
                                open_params.text_document.uri.clone(),
                                open_params.text_document.text.clone(),
                            );
                            publish_diagnostics(
                                &open_params.text_document.uri,
                                &open_params.text_document.text,
                                &mut writer,
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
                                    change_params.text_document.uri.clone(),
                                    change.text.clone(),
                                );
                                publish_diagnostics(
                                    &change_params.text_document.uri,
                                    &change.text,
                                    &mut writer,
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
                            let clear_params = serde_json::json!({
                                "uri": close_params.text_document.uri,
                                "diagnostics": []
                            });
                            send_notification(&mut writer, "textDocument/publishDiagnostics", Some(clear_params));
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

struct TimeEntryParts<'a> {
    is_range: bool,
    has_leading_bracket: bool,
    start_time: &'a str,
    end_time: Option<&'a str>,
    has_trailing_bracket: bool,
    duration: Option<&'a str>,
    rest: &'a str,
}

fn parse_time_entry_line(line: &str) -> Option<TimeEntryParts<'_>> {
    if !line.starts_with("    ") || line.starts_with("     ") {
        return None;
    }
    let content = &line[4..];
    if content.is_empty() {
        return None;
    }

    // Check if it starts with '<'
    let has_leading_bracket = content.starts_with('<');
    let s = if has_leading_bracket {
        &content[1..]
    } else {
        content
    };

    // Find the first token (start_time or duration)
    let mut chars = s.char_indices().peekable();
    let mut end_token1 = 0;
    while let Some(&(idx, c)) = chars.peek() {
        if c.is_whitespace() || (c == '-' && idx > 0) {
            break;
        }
        end_token1 = idx + c.len_utf8();
        chars.next();
    }

    if end_token1 == 0 {
        return None;
    }

    let token1 = &s[..end_token1];
    let rest = &s[end_token1..];

    if token1.contains(':') {
        // It's a time range start. Let's find the '-'
        let mut rest_chars = rest.char_indices().peekable();
        let mut hyphen_idx = None;
        while let Some(&(idx, c)) = rest_chars.peek() {
            if c == '-' {
                hyphen_idx = Some(idx);
                break;
            } else if !c.is_whitespace() {
                // If we encounter a non-whitespace character before a hyphen, it's not a valid time range
                return None;
            }
            rest_chars.next();
        }

        if let Some(h_idx) = hyphen_idx {
            let after_hyphen = &rest[h_idx + 1..];
            let end_time_start = after_hyphen.trim_start();
            let mut end_time_chars = end_time_start.char_indices().peekable();
            let mut end_token_len = 0;
            while let Some(&(idx, c)) = end_time_chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                end_token_len = idx + c.len_utf8();
                end_time_chars.next();
            }

            if end_token_len == 0 {
                return None;
            }

            let mut end_token = &end_time_start[..end_token_len];
            let has_trailing_bracket = end_token.ends_with('>');
            if has_trailing_bracket {
                end_token = &end_token[..end_token.len() - 1];
            }

            if end_token == "?" || end_token.contains(':') {
                let rest_after_range = &end_time_start[end_token_len..];
                return Some(TimeEntryParts {
                    is_range: true,
                    has_leading_bracket,
                    start_time: token1,
                    end_time: Some(end_token),
                    has_trailing_bracket,
                    duration: None,
                    rest: rest_after_range.trim_start(),
                });
            }
        }
    } else {
        // Check if it's a duration
        if parse_duration_to_minutes(token1).is_some() {
            return Some(TimeEntryParts {
                is_range: false,
                has_leading_bracket: false,
                start_time: "",
                end_time: None,
                has_trailing_bracket: false,
                duration: Some(token1),
                rest: rest.trim_start(),
            });
        }
    }

    None
}

fn split_tags_and_description(rest: &str) -> (String, String) {
    let mut tags = Vec::new();
    let mut description_start_idx = rest.len();
    let mut temp_s = rest;

    while !temp_s.is_empty() {
        let trimmed = temp_s.trim_start();
        if trimmed.starts_with('#') {
            let word_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let tag = &trimmed[..word_len];
            tags.push(tag);
            temp_s = &trimmed[word_len..];
        } else {
            description_start_idx = rest.len() - temp_s.len();
            break;
        }
    }

    let tags_str = tags.join(" ");
    let desc_str = rest[description_start_idx..].trim().to_string();
    (tags_str, desc_str)
}

fn align_klog_content(content: &str) -> String {
    let mut blocks = Vec::new();
    let mut current_block = Vec::new();

    for line in content.lines() {
        if !line.is_empty() && !line.starts_with(char::is_whitespace) {
            if !current_block.is_empty() {
                blocks.push(current_block);
                current_block = Vec::new();
            }
        }
        current_block.push(line);
    }
    if !current_block.is_empty() {
        blocks.push(current_block);
    }

    let mut aligned_content = String::new();

    for block in blocks {
        enum BlockLine<'a> {
            TimeEntry(ParsedTimeEntry<'a>),
            Raw(&'a str),
        }

        struct ParsedTimeEntry<'a> {
            is_range: bool,
            has_leading_bracket: bool,
            start_time: &'a str,
            end_time: Option<&'a str>,
            has_trailing_bracket: bool,
            duration: Option<&'a str>,
            tags_str: String,
            desc_str: String,
        }

        let mut parsed_lines = Vec::new();
        let mut max_time_str_len = 0;
        let mut max_tags_len = 0;

        for &line in &block {
            if let Some(parts) = parse_time_entry_line(line) {
                let (tags_str, desc_str) = split_tags_and_description(parts.rest);
                max_tags_len = std::cmp::max(max_tags_len, tags_str.len());

                parsed_lines.push(BlockLine::TimeEntry(ParsedTimeEntry {
                    is_range: parts.is_range,
                    has_leading_bracket: parts.has_leading_bracket,
                    start_time: parts.start_time,
                    end_time: parts.end_time,
                    has_trailing_bracket: parts.has_trailing_bracket,
                    duration: parts.duration,
                    tags_str,
                    desc_str,
                }));
            } else {
                parsed_lines.push(BlockLine::Raw(line));
            }
        }

        for pline in &parsed_lines {
            if let BlockLine::TimeEntry(ref entry) = pline {
                let start_part = if entry.has_leading_bracket {
                    format!("<{}", entry.start_time)
                } else {
                    entry.start_time.to_string()
                };

                let end_part = if let Some(ref et) = entry.end_time {
                    if entry.has_trailing_bracket {
                        format!("{}>", et)
                    } else {
                        et.to_string()
                    }
                } else {
                    "".to_string()
                };

                let time_str = if entry.is_range {
                    format!("{} - {}", start_part, end_part)
                } else {
                    entry.duration.as_ref().unwrap().to_string()
                };

                max_time_str_len = std::cmp::max(max_time_str_len, time_str.len());
            }
        }

        let tags_start_col = max_time_str_len + 3;
        let desc_start_col = max_tags_len + 2;

        for pline in parsed_lines {
            match pline {
                BlockLine::Raw(line) => {
                    aligned_content.push_str(line);
                    aligned_content.push('\n');
                }
                BlockLine::TimeEntry(entry) => {
                    let start_part = if entry.has_leading_bracket {
                        format!("<{}", entry.start_time)
                    } else {
                        entry.start_time.to_string()
                    };

                    let end_part = if let Some(et) = entry.end_time {
                        if entry.has_trailing_bracket {
                            format!("{}>", et)
                        } else {
                            et.to_string()
                        }
                    } else {
                        "".to_string()
                    };

                    let time_str = if entry.is_range {
                        format!("{} - {}", start_part, end_part)
                    } else {
                        entry.duration.as_ref().unwrap().to_string()
                    };

                    let time_padded = format!("{:width$}", time_str, width = tags_start_col);
                    let mut formatted_line = format!("    {}", time_padded);

                    if max_tags_len > 0 {
                        let tags_padded = format!("{:width$}", entry.tags_str, width = desc_start_col);
                        if !entry.desc_str.is_empty() {
                            formatted_line.push_str(&format!("{}{}", tags_padded, entry.desc_str));
                        } else if !entry.tags_str.is_empty() {
                            formatted_line.push_str(&entry.tags_str);
                        }
                    } else {
                        if !entry.desc_str.is_empty() {
                            formatted_line.push_str(&entry.desc_str);
                        }
                    }
                    let clean_line = formatted_line.trim_end();
                    aligned_content.push_str(clean_line);
                    aligned_content.push('\n');
                }
            }
        }
    }
    aligned_content
}

fn get_folding_ranges(content: &str) -> Vec<FoldingRange> {
    let mut folding_ranges = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // 1. Day records folding ranges
    let mut record_start_line: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if !line.is_empty() && !line.starts_with(char::is_whitespace) {
            if let Some(start) = record_start_line {
                let mut end = idx - 1;
                while end > start && lines[end].trim().is_empty() {
                    end -= 1;
                }
                if end > start {
                    folding_ranges.push(FoldingRange {
                        start_line: start as u32,
                        end_line: end as u32,
                        kind: Some("region".to_string()),
                    });
                }
            }
            record_start_line = Some(idx);
        }
    }
    if let Some(start) = record_start_line {
        let mut end = lines.len().saturating_sub(1);
        while end > start && lines[end].trim().is_empty() {
            end -= 1;
        }
        if end > start {
            folding_ranges.push(FoldingRange {
                start_line: start as u32,
                end_line: end as u32,
                kind: Some("region".to_string()),
            });
        }
    }

    // 2. Time entries folding ranges
    let mut entry_start_line: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if parse_time_entry_line(line).is_some() {
            if let Some(start) = entry_start_line {
                let mut end = idx - 1;
                while end > start && lines[end].trim().is_empty() {
                    end -= 1;
                }
                if end > start {
                    folding_ranges.push(FoldingRange {
                        start_line: start as u32,
                        end_line: end as u32,
                        kind: Some("region".to_string()),
                    });
                }
            }
            entry_start_line = Some(idx);
        } else if !line.trim().is_empty() {
            if line.starts_with("     ") || line.starts_with("    \t") || line.starts_with("\t") {
                // Continuation line, keep tracking
            } else {
                if let Some(start) = entry_start_line {
                    let mut end = idx - 1;
                    while end > start && lines[end].trim().is_empty() {
                        end -= 1;
                    }
                    if end > start {
                        folding_ranges.push(FoldingRange {
                            start_line: start as u32,
                            end_line: end as u32,
                            kind: Some("region".to_string()),
                        });
                    }
                    entry_start_line = None;
                }
            }
        }
    }
    if let Some(start) = entry_start_line {
        let mut end = lines.len().saturating_sub(1);
        while end > start && lines[end].trim().is_empty() {
            end -= 1;
        }
        if end > start {
            folding_ranges.push(FoldingRange {
                start_line: start as u32,
                end_line: end as u32,
                kind: Some("region".to_string()),
            });
        }
    }

    folding_ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29); // Leap year
        assert_eq!(days_in_month(2000, 2), 29); // Leap year
        assert_eq!(days_in_month(1900, 2), 28); // Not leap year
        assert_eq!(days_in_month(2026, 4), 30);
    }

    #[test]
    fn test_find_latest_date_in_content() {
        let content = "2026-05-10\n  1h #project=Alpha\n\n2026-05-20\n  2h #project=Beta";
        assert_eq!(find_latest_date_in_content(content), Some((2026, 5, 20)));

        let content_slashes = "2026/04/15\n  1h\n2026/04/25\n  2h";
        assert_eq!(find_latest_date_in_content(content_slashes), Some((2026, 4, 25)));

        let content_empty = "no dates here";
        assert_eq!(find_latest_date_in_content(content_empty), None);
    }

    #[test]
    fn test_parse_duration_to_minutes() {
        assert_eq!(parse_duration_to_minutes("1h30m"), Some(90));
        assert_eq!(parse_duration_to_minutes("45m"), Some(45));
        assert_eq!(parse_duration_to_minutes("2h"), Some(120));
        assert_eq!(parse_duration_to_minutes("+1h15m"), Some(75));
        assert_eq!(parse_duration_to_minutes("-30m"), Some(-30));
        assert_eq!(parse_duration_to_minutes("8h!"), Some(480));
    }

    #[test]
    fn test_format_minutes_to_duration() {
        assert_eq!(format_minutes_to_duration(90), "1h30m");
        assert_eq!(format_minutes_to_duration(45), "45m");
        assert_eq!(format_minutes_to_duration(120), "2h");
        assert_eq!(format_minutes_to_duration(-30), "-30m");
        assert_eq!(format_minutes_to_duration(0), "0m");
    }

    #[test]
    fn test_parse_duration_to_minutes_flexible() {
        assert_eq!(parse_duration_to_minutes_flexible("7.7h"), Some(462));
        assert_eq!(parse_duration_to_minutes_flexible("8h"), Some(480));
        assert_eq!(parse_duration_to_minutes_flexible("7h30m"), Some(450));
        assert_eq!(parse_duration_to_minutes_flexible("450m"), Some(450));
        assert_eq!(parse_duration_to_minutes_flexible("7.5"), Some(450));
        assert_eq!(parse_duration_to_minutes_flexible("8"), Some(480));
        assert_eq!(parse_duration_to_minutes_flexible("450"), Some(450));
        assert_eq!(parse_duration_to_minutes_flexible(""), None);
    }

    #[test]
    fn test_parse_day_duration_setting() {
        assert_eq!(parse_day_duration_setting(&serde_json::json!(7.7)), Some(462));
        assert_eq!(parse_day_duration_setting(&serde_json::json!(8)), Some(480));
        assert_eq!(parse_day_duration_setting(&serde_json::json!(450)), Some(450));
        assert_eq!(parse_day_duration_setting(&serde_json::json!("7.7h")), Some(462));
        assert_eq!(parse_day_duration_setting(&serde_json::json!("8h")), Some(480));
        assert_eq!(parse_day_duration_setting(&serde_json::json!("7h30m")), Some(450));
    }

    #[test]
    fn test_find_day_duration_recursively() {
        let value = serde_json::json!({
            "settings": {
                "klog-lsp": {
                    "day_duration": "7.7h"
                }
            }
        });
        assert_eq!(find_day_duration_recursively(&value), Some(serde_json::json!("7.7h")));

        let val_direct = serde_json::json!({
            "day_duration": 8
        });
        assert_eq!(find_day_duration_recursively(&val_direct), Some(serde_json::json!(8)));
    }

    #[test]
    fn test_working_days_in_month() {
        // May 2026 has 31 days, starts on Friday, ends on Sunday. 10 weekend days. 21 working days.
        assert_eq!(working_days_in_month(2026, 5), 21);
        // Feb 2026 has 28 days, starts on Sunday. 8 weekend days. 20 working days.
        assert_eq!(working_days_in_month(2026, 2), 20);
        // Jan 2026 has 31 days, starts on Thursday. 9 weekend days. 22 working days.
        assert_eq!(working_days_in_month(2026, 1), 22);
    }

    #[test]
    fn test_proportional_project_estimations() {
        // May 2026 has 21 working days.
        // If we set day_duration to 7h42m (462 minutes), the total end-of-month target is 21 * 462 = 9702 minutes (21d).
        // If total tracked is 5h (300 mins), Project Alpha has 2h (120 mins) and Project Beta has 3h (180 mins).
        // Ratio = 9702 / 300 = 32.34.
        // Project Alpha est: 120 * 32.34 = 3880.8 => 3881 mins => 8.4d.
        // Project Beta est: 180 * 32.34 = 5821.2 => 5821 mins => 12.6d.
        // Total est: 21d.
        let content = "2026-05-20 (8h!)\n  9:00 - 11:00 #project=Alpha\n  11:00 - 14:00 #project=Beta\n";
        
        // Reset/make sure day duration is at default (7h42m = 462 mins)
        DAY_DURATION_MINUTES.store(462, std::sync::atomic::Ordering::Relaxed);

        let breakdown = get_project_aligned_text(content).unwrap();
        println!("Aligned project breakdown:\n{}", breakdown);
        
        // Verify output table rows are present and aligned properly
        assert!(breakdown.contains("#project=Alpha"));
        assert!(breakdown.contains("#project=Beta"));
        assert!(breakdown.contains("2h (40.0%)"));
        assert!(breakdown.contains("3h (60.0%)"));
        assert!(breakdown.contains("8.4d  (40.0%)"));
        assert!(breakdown.contains("12.6d (60.0%)"));
        assert!(breakdown.contains("21d   (100.0%)"));
        assert!(breakdown.contains("5h (100.0%)"));

        // Let's also verify the project hover report
        let report = get_project_table_report(content).unwrap();
        println!("Project table report:\n{}", report);
        assert!(report.contains("`#project=Alpha`"));
        assert!(report.contains("`#project=Beta`"));
        assert!(report.contains("2h (40.0%)"));
        assert!(report.contains("3h (60.0%)"));
        assert!(report.contains("8.4d  (40.0%)"));
        assert!(report.contains("12.6d (60.0%)"));
        assert!(report.contains("21d   (100.0%)"));
        assert!(report.contains("5h (100.0%)"));
    }

    #[test]
    fn test_non_estimated_projects() {
        // May 2026 has 21 working days. Total target capacity = 21 * 7h42m = 21d.
        // Total tracked = 5h.
        // Projects: ABSCP (2h), Alpha (3h).
        // Ratio = 21d / 5h = 9702 mins / 300 mins = 32.34.
        // Alpha (ratio-estimated): 3h * 32.34 = 97.02h = 5821.2 mins => 12.6d.
        // ABSCP (no ratio estimation, stays same value): 2h.
        let content = "2026-05-20 (8h!)\n  9:00 - 11:00 #project=ABSCP\n  11:00 - 14:00 #project=Alpha\n";
        
        DAY_DURATION_MINUTES.store(462, std::sync::atomic::Ordering::Relaxed);

        let breakdown = get_project_aligned_text(content).unwrap();
        println!("Aligned project breakdown with non-estimated:\n{}", breakdown);
        
        assert!(breakdown.contains("#project=ABSCP"));
        assert!(breakdown.contains("#project=Alpha"));
        assert!(breakdown.contains("2h (40.0%)")); // Both columns should have 2h for ABSCP
        assert!(breakdown.contains("12.6d (60.0%)")); // Alpha is estimated

        // Verify the project hover report too
        let report = get_project_table_report(content).unwrap();
        println!("Project table report with non-estimated:\n{}", report);
        assert!(report.contains("`#project=ABSCP`"));
        assert!(report.contains("`#project=Alpha`"));
        assert!(report.contains("2h (40.0%)"));
        assert!(report.contains("12.6d (60.0%)"));
    }

    #[test]
    fn test_current_date_and_time_helpers() {
        if let Some(date) = get_current_date() {
            assert_eq!(date.len(), 10);
            let parts: Vec<&str> = date.split('-').collect();
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0].len(), 4);
            assert_eq!(parts[1].len(), 2);
            assert_eq!(parts[2].len(), 2);
        }
        if let Some(time) = get_current_time() {
            assert_eq!(time.len(), 5);
            let parts: Vec<&str> = time.split(':').collect();
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0].len(), 2);
            assert_eq!(parts[1].len(), 2);
        }
    }

    #[test]
    fn test_get_completions() {
        // Test with default day duration (7h42m = 462 mins)
        DAY_DURATION_MINUTES.store(462, std::sync::atomic::Ordering::Relaxed);
        let completions_default = get_completions("2026-05-21", "10:14", None);
        let list_default = completions_default.as_array().expect("completions should be a list");
        
        let find_insert_text = |list: &[serde_json::Value], lbl: &str| -> String {
            list.iter()
                .find(|item| item["label"].as_str().unwrap() == lbl)
                .and_then(|item| item["insertText"].as_str())
                .unwrap()
                .to_string()
        };

        assert_eq!(find_insert_text(list_default, "today"), "${1:2026-05-21} (${2:7h42m!})");
        assert_eq!(find_insert_text(list_default, "date"), "${1:YYYY-MM-DD} (${2:7h42m!})");
        assert_eq!(find_insert_text(list_default, "20"), "${1:2026-05-21} (${2:7h42m!})");
        assert_eq!(find_insert_text(list_default, "2026-05-21"), "${1:2026-05-21} (${2:7h42m!})");
        assert_eq!(find_insert_text(list_default, "record"), "${1:2026-05-21} (${2:7h42m!})\n${3:Summary}\n    $0");

        // Test with custom day duration (8h = 480 mins)
        DAY_DURATION_MINUTES.store(480, std::sync::atomic::Ordering::Relaxed);
        let completions_8h = get_completions("2026-05-21", "10:14", None);
        let list_8h = completions_8h.as_array().expect("completions should be a list");

        assert_eq!(find_insert_text(list_8h, "today"), "${1:2026-05-21} (${2:8h!})");
        assert_eq!(find_insert_text(list_8h, "date"), "${1:YYYY-MM-DD} (${2:8h!})");
        assert_eq!(find_insert_text(list_8h, "20"), "${1:2026-05-21} (${2:8h!})");
        assert_eq!(find_insert_text(list_8h, "2026-05-21"), "${1:2026-05-21} (${2:8h!})");
        assert_eq!(find_insert_text(list_8h, "record"), "${1:2026-05-21} (${2:8h!})\n${3:Summary}\n    $0");
    }

    #[test]
    fn test_find_open_range_question_mark() {
        assert_eq!(find_open_range_question_mark("    10:00 - ?"), Some(12));
        assert_eq!(find_open_range_question_mark("    10:00-?"), Some(10));
        assert_eq!(find_open_range_question_mark("    <10:00 - ?"), Some(13));
        assert_eq!(find_open_range_question_mark("    10:00 - 11:00 Did something?"), None);
        assert_eq!(find_open_range_question_mark("    not a time entry - ?"), None);
    }

    #[test]
    fn test_get_code_actions() {
        let content = "2026-05-21 (8h!)\n  9:00 - 10:00\n  10:00 - ?\n";
        let uri = "file:///test.klg";
        
        let actions = get_code_actions(
            uri,
            content,
            Range {
                start: Position { line: 1, character: 5 },
                end: Position { line: 1, character: 5 },
            },
        );
        
        assert_eq!(actions.len(), 2);
        
        let start_action = actions.iter().find(|a| a.title.starts_with("Start open-ended time entry")).unwrap();
        let stop_action = actions.iter().find(|a| a.title.starts_with("Stop active timer")).unwrap();
        
        let start_edit = start_action.edit.as_ref().unwrap().changes.as_ref().unwrap().get(uri).unwrap();
        assert_eq!(start_edit.len(), 1);
        assert_eq!(start_edit[0].range.start.line, 1);
        assert_eq!(start_edit[0].range.start.character, 14);
        
        let stop_edit = stop_action.edit.as_ref().unwrap().changes.as_ref().unwrap().get(uri).unwrap();
        assert_eq!(stop_edit.len(), 1);
        assert_eq!(stop_edit[0].range.start.line, 2);
        assert_eq!(stop_edit[0].range.start.character, 10);
    }

    #[test]
    fn test_extract_tags() {
        let content = "2026-05-21\n  10:00 - 11:00 #work #project=Alpha\n  11:00 - 12:00 #meeting #project=Beta";
        let tags = extract_tags(content);
        assert_eq!(tags, vec![
            "#meeting".to_string(),
            "#project=".to_string(),
            "#project=Alpha".to_string(),
            "#project=Beta".to_string(),
            "#work".to_string(),
        ]);
    }

    #[test]
    fn test_find_klog_path_recursively() {
        let value = serde_json::json!({
            "settings": {
                "klog-lsp": {
                    "klog_path": "/usr/local/bin/klog"
                }
            }
        });
        assert_eq!(find_klog_path_recursively(&value), Some("/usr/local/bin/klog".to_string()));

        let val_direct = serde_json::json!({
            "klog_path": "klog-custom"
        });
        assert_eq!(find_klog_path_recursively(&val_direct), Some("klog-custom".to_string()));
    }

    #[test]
    fn test_parse_time_entry_line() {
        let p1 = parse_time_entry_line("    8:15 - 8:30 #project=RT Cost analysis").unwrap();
        assert!(p1.is_range);
        assert_eq!(p1.start_time, "8:15");
        assert_eq!(p1.end_time, Some("8:30"));
        assert_eq!(p1.rest, "#project=RT Cost analysis");
        assert!(!p1.has_leading_bracket);
        assert!(!p1.has_trailing_bracket);

        let p2 = parse_time_entry_line("    <10:00 - ?").unwrap();
        assert!(p2.is_range);
        assert_eq!(p2.start_time, "10:00");
        assert_eq!(p2.end_time, Some("?"));
        assert!(p2.has_leading_bracket);
        assert!(!p2.has_trailing_bracket);

        let p3 = parse_time_entry_line("    10:00 - 11:00> #tag").unwrap();
        assert!(p3.is_range);
        assert_eq!(p3.start_time, "10:00");
        assert_eq!(p3.end_time, Some("11:00"));
        assert!(!p3.has_leading_bracket);
        assert!(p3.has_trailing_bracket);

        let p4 = parse_time_entry_line("    1h30m #project=A").unwrap();
        assert!(!p4.is_range);
        assert_eq!(p4.duration, Some("1h30m"));
        assert_eq!(p4.rest, "#project=A");

        let p5 = parse_time_entry_line("    -45m!").unwrap();
        assert!(!p5.is_range);
        assert_eq!(p5.duration, Some("-45m!"));
        assert_eq!(p5.rest, "");

        assert!(parse_time_entry_line("  8:15 - 8:30").is_none());
        assert!(parse_time_entry_line("     8:15 - 8:30").is_none());
        assert!(parse_time_entry_line("    not a time entry").is_none());
    }

    #[test]
    fn test_split_tags_and_description() {
        let (t1, d1) = split_tags_and_description("#project=RT Cost analysis");
        assert_eq!(t1, "#project=RT");
        assert_eq!(d1, "Cost analysis");

        let (t2, d2) = split_tags_and_description("#project=VORTEX#ticket=glpi-260 Deploy new resources");
        assert_eq!(t2, "#project=VORTEX#ticket=glpi-260");
        assert_eq!(d2, "Deploy new resources");

        let (t3, d3) = split_tags_and_description("#tag1 #tag2   Some description");
        assert_eq!(t3, "#tag1 #tag2");
        assert_eq!(d3, "Some description");

        let (t4, d4) = split_tags_and_description("No tags here");
        assert_eq!(t4, "");
        assert_eq!(d4, "No tags here");
    }

    #[test]
    fn test_align_klog_content() {
        let input = "\
2026-05-21 (7h42m!) | Total: 6h | Should: 1d! | Diff: -1h42m
    8:15 - 8:30 #project=RT Cost analysis
    10:00 - 10:40 #project=VORTEX#ticket=glpi-260 Fix deployment issues
    11:45 - 12:15 #project=VORTEX Project cleanup and push changes
    14:00 - 15:00 #project=SINCRONE#lot=2-5 COSUI
    1h30m #project=RT Duration entry
    45m
";
        let expected = "\
2026-05-21 (7h42m!) | Total: 6h | Should: 1d! | Diff: -1h42m
    8:15 - 8:30     #project=RT                      Cost analysis
    10:00 - 10:40   #project=VORTEX#ticket=glpi-260  Fix deployment issues
    11:45 - 12:15   #project=VORTEX                  Project cleanup and push changes
    14:00 - 15:00   #project=SINCRONE#lot=2-5        COSUI
    1h30m           #project=RT                      Duration entry
    45m
";
        let result = align_klog_content(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_get_folding_ranges() {
        let content = "\
2026-05-21 (8h!)
    8:15 - 8:30 #project=RT
        Cost analysis
        details here

    10:00 - 10:40 #project=VORTEX

2026-05-22
    9:00 - 10:00
";
        let ranges = get_folding_ranges(content);
        assert_eq!(ranges.len(), 3);
        assert!(ranges.contains(&FoldingRange {
            start_line: 0,
            end_line: 5,
            kind: Some("region".to_string()),
        }));
        assert!(ranges.contains(&FoldingRange {
            start_line: 1,
            end_line: 3,
            kind: Some("region".to_string()),
        }));
        assert!(ranges.contains(&FoldingRange {
            start_line: 7,
            end_line: 8,
            kind: Some("region".to_string()),
        }));
    }
}

