//! File system tools: read_file, write, search_replace, list_dir, grep, glob.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{short_target, Tool, ToolContext, ToolKind, ToolResult};

pub const READ_LINE_LIMIT: usize = 2000;
pub const READ_BYTE_LIMIT: usize = 256 * 1024;
pub const GREP_MATCH_LIMIT: usize = 250;

fn resolve(ctx: &ToolContext, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.cwd.join(p)
    }
}

// ---------------------------------------------------------------------------
// read_file
// ---------------------------------------------------------------------------

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a file from the filesystem with line numbers. Supports offset/limit \
         for large files. Returns an error for binary files."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file (absolute or relative to cwd)" },
                "offset": { "type": "integer", "description": "1-based line to start at" },
                "limit": { "type": "integer", "description": "Maximum number of lines to read" }
            },
            "required": ["file_path"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn title(&self, args: &Value) -> String {
        format!("Read {}", short_target(args, &["file_path"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(raw) = args.get("file_path").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: file_path");
        };
        let path = resolve(ctx, raw);
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(READ_LINE_LIMIT as u64) as usize;

        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(e) => return ToolResult::err(format!("cannot read {}: {e}", path.display())),
        };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            return ToolResult::err(format!("{} appears to be a binary file", path.display()));
        }
        let text = String::from_utf8_lossy(&bytes);
        let truncated_bytes = text.len() > READ_BYTE_LIMIT;
        let text_ref: &str = if truncated_bytes {
            &text[..READ_BYTE_LIMIT.min(text.len())]
        } else {
            &text
        };

        let mut out = String::new();
        let total_lines = text_ref.lines().count();
        for (i, line) in text_ref.lines().enumerate().skip(offset - 1).take(limit) {
            out.push_str(&format!("{:>6}\t{}\n", i + 1, line));
        }
        if truncated_bytes {
            out.push_str("... [file truncated]\n");
        }
        if out.is_empty() {
            out =
                format!("(file is empty or offset {offset} is past the end; {total_lines} lines)");
        }
        ToolResult::ok(out)
    }
}

// ---------------------------------------------------------------------------
// write
// ---------------------------------------------------------------------------

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }
    fn description(&self) -> &str {
        "Write content to a file, creating parent directories as needed. \
         Overwrites the file when it exists. Prefer search_replace for edits."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to write" },
                "content": { "type": "string", "description": "Full file content" }
            },
            "required": ["file_path", "content"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Write
    }
    fn title(&self, args: &Value) -> String {
        format!("Write {}", short_target(args, &["file_path"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(raw) = args.get("file_path").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: file_path");
        };
        let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: content");
        };
        let path = resolve(ctx, raw);
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult::err(format!("cannot create {}: {e}", parent.display()));
            }
        }
        let existed = path.exists();
        let old = if existed {
            tokio::fs::read_to_string(&path).await.ok()
        } else {
            None
        };
        if let Err(e) = tokio::fs::write(&path, content).await {
            return ToolResult::err(format!("cannot write {}: {e}", path.display()));
        }
        let added = count_added(old.as_deref(), content);
        let meta = json!({
            "path": path,
            "created": !existed,
            "added": added,
            "removed": count_lines(old.as_deref()) as i64 - added,
        });
        ToolResult::with_meta(
            format!(
                "{} {} ({} lines)",
                if existed { "Updated" } else { "Created" },
                path.display(),
                content.lines().count()
            ),
            meta,
        )
    }
}

// ---------------------------------------------------------------------------
// search_replace
// ---------------------------------------------------------------------------

pub struct SearchReplaceTool;

#[async_trait]
impl Tool for SearchReplaceTool {
    fn name(&self) -> &str {
        "search_replace"
    }
    fn description(&self) -> &str {
        "Make a precise edit: replace an exact string in a file. old_string must \
         match exactly (including whitespace) and must be unique unless replace_all."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path of the file to edit" },
                "old_string": { "type": "string", "description": "Exact text to replace" },
                "new_string": { "type": "string", "description": "Replacement text" },
                "replace_all": { "type": "boolean", "description": "Replace every occurrence" }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Edit
    }
    fn title(&self, args: &Value) -> String {
        format!("Edit {}", short_target(args, &["file_path"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let (Some(raw), Some(old), Some(new)) = (
            args.get("file_path").and_then(|v| v.as_str()),
            args.get("old_string").and_then(|v| v.as_str()),
            args.get("new_string").and_then(|v| v.as_str()),
        ) else {
            return ToolResult::err("missing file_path, old_string or new_string");
        };
        if old == new {
            return ToolResult::err("old_string and new_string are identical");
        }
        let path = resolve(ctx, raw);
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("cannot read {}: {e}", path.display())),
        };
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let count = content.matches(old).count();
        if count == 0 {
            return ToolResult::err(format!(
                "old_string not found in {} (read the file and copy the exact text)",
                path.display()
            ));
        }
        if count > 1 && !replace_all {
            return ToolResult::err(format!(
                "old_string appears {count} times in {}; add more context to make it unique or set replace_all",
                path.display()
            ));
        }
        let updated = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        if let Err(e) = tokio::fs::write(&path, &updated).await {
            return ToolResult::err(format!("cannot write {}: {e}", path.display()));
        }
        let added = count_added(Some(&content), &updated);
        let removed =
            count_lines(Some(&content)) as i64 - count_lines(Some(&updated)) as i64 + added;
        let meta = json!({
            "path": path,
            "added": added,
            "removed": removed.max(0),
            "diff": build_diff(&content, &updated, &path),
            "replacements": if replace_all { count } else { 1 },
        });
        ToolResult::with_meta(
            format!(
                "Edited {} ({} replacement{}, +{added} lines)",
                path.display(),
                if replace_all { count } else { 1 },
                if count == 1 { "" } else { "s" }
            ),
            meta,
        )
    }
}

fn count_lines(s: Option<&str>) -> usize {
    s.map(|s| s.lines().count()).unwrap_or(0)
}

fn count_added(old: Option<&str>, new: &str) -> i64 {
    match old {
        None => new.lines().count() as i64,
        Some(old) => {
            let old_set: Vec<&str> = old.lines().collect();
            let new_lines: Vec<&str> = new.lines().collect();
            // Coarse added-line estimate using a simple LCS-free heuristic.
            let mut added = 0i64;
            let mut j = 0;
            for line in &new_lines {
                if j < old_set.len() && old_set[j] == *line {
                    j += 1;
                } else if old_set.contains(line) && new_lines.len() < old_set.len() {
                    // likely moved/removed; skip counting
                } else {
                    added += 1;
                }
            }
            added
        }
    }
}

/// Produce a compact unified diff preview for the TUI.
pub fn build_diff(old: &str, new: &str, path: &Path) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = format!("--- {}\n+++ {}\n", path.display(), path.display());
    let max_context = 3;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut last_emitted_old = 0usize;
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
            i += 1;
            j += 1;
            continue;
        }
        // find resync point
        let start_old = i;
        let start_new = j;
        let mut resync: Option<(usize, usize)> = None;
        'outer: for (oi, old_line) in old_lines
            .iter()
            .enumerate()
            .take((i + 40).min(old_lines.len()))
            .skip(i)
        {
            for (nj, new_line) in new_lines
                .iter()
                .enumerate()
                .take((j + 40).min(new_lines.len()))
                .skip(j)
            {
                if old_line == new_line {
                    resync = Some((oi, nj));
                    break 'outer;
                }
            }
        }
        let (end_old, end_new) = resync.unwrap_or((old_lines.len(), new_lines.len()));
        // context before
        let ctx_start = start_old.saturating_sub(max_context).max(last_emitted_old);
        for k in ctx_start..start_old {
            if k < old_lines.len() {
                out.push_str(&format!(" {}\n", old_lines[k]));
            }
        }
        for line in &old_lines[start_old..end_old] {
            out.push_str(&format!("-{line}\n"));
        }
        for line in &new_lines[start_new..end_new] {
            out.push_str(&format!("+{line}\n"));
        }
        last_emitted_old = end_old;
        i = end_old;
        j = end_new;
        if resync.is_none() {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// list_dir
// ---------------------------------------------------------------------------

pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "List the entries of a directory (names, sizes, directories marked with /)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path (default: cwd)" }
            }
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Search
    }
    fn title(&self, args: &Value) -> String {
        format!("List {}", short_target(args, &["path"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve(ctx, p))
            .unwrap_or_else(|| ctx.cwd.clone());
        let mut entries = Vec::new();
        let mut rd = match tokio::fs::read_dir(&path).await {
            Ok(rd) => rd,
            Err(e) => return ToolResult::err(format!("cannot list {}: {e}", path.display())),
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
            entries.push((name, is_dir, size));
        }
        entries.sort_by(|a, b| {
            (b.1, a.0.to_lowercase())
                .cmp(&(a.1, b.0.to_lowercase()))
                .reverse()
        });
        entries.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .reverse()
                .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        let mut out = String::new();
        for (name, is_dir, size) in &entries {
            if *is_dir {
                out.push_str(&format!("{name}/\n"));
            } else {
                out.push_str(&format!("{name}  ({} bytes)\n", size));
            }
        }
        if out.is_empty() {
            out = "(empty directory)".into();
        }
        ToolResult::ok(out)
    }
}

// ---------------------------------------------------------------------------
// grep
// ---------------------------------------------------------------------------

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn display_name(&self) -> String {
        "grep".into()
    }
    fn description(&self) -> &str {
        "Search file contents with a regular expression (ripgrep-style). \
         Respects .gitignore. Returns path:line:match for each hit."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regular expression" },
                "path": { "type": "string", "description": "File or directory to search (default: cwd)" },
                "glob": { "type": "string", "description": "Limit files, e.g. '*.rs' or 'src/**/*.ts'" },
                "case_insensitive": { "type": "boolean" },
                "files_with_matches": { "type": "boolean", "description": "Only list matching files" },
                "max_results": { "type": "integer" }
            },
            "required": ["pattern"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Search
    }
    fn title(&self, args: &Value) -> String {
        format!("Search \"{}\"", short_target(args, &["pattern"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: pattern");
        };
        let ci = args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let files_only = args
            .get("files_with_matches")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let limit = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(GREP_MATCH_LIMIT as u64) as usize;
        let re = match regex::RegexBuilder::new(pattern)
            .case_insensitive(ci)
            .build()
        {
            Ok(re) => re,
            Err(e) => return ToolResult::err(format!("invalid regex: {e}")),
        };
        let base = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve(ctx, p))
            .unwrap_or_else(|| ctx.cwd.clone());

        let mut builder = ignore::WalkBuilder::new(&base);
        builder
            .hidden(false)
            .git_ignore(ctx.config.tools.respect_gitignore);
        if let Some(g) = args.get("glob").and_then(|v| v.as_str()) {
            let mut ob = ignore::overrides::OverrideBuilder::new(&base);
            if ob.add(g).is_ok() {
                if let Ok(ov) = ob.build() {
                    builder.overrides(ov);
                }
            }
        }
        let walker = builder.build();

        let mut hits = Vec::new();
        let mut files = std::collections::BTreeSet::new();
        let mut file_cache = std::collections::HashMap::new();
        for entry in walker.flatten() {
            if hits.len() >= limit {
                break;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) if m.len() < 2_000_000 => m,
                _ => continue,
            };
            let _ = meta;
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for (lineno, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    if files_only {
                        files.insert(path.display().to_string());
                    } else {
                        let trimmed = line.trim_end();
                        let display_line: String = trimmed.chars().take(240).collect();
                        hits.push(format!(
                            "{}:{}:{}",
                            path.display(),
                            lineno + 1,
                            display_line
                        ));
                    }
                    if hits.len() >= limit {
                        break;
                    }
                }
            }
            file_cache.insert(path.to_path_buf(), ());
        }
        if files_only {
            if files.is_empty() {
                return ToolResult::ok("no matches");
            }
            return ToolResult::ok(files.into_iter().collect::<Vec<_>>().join("\n"));
        }
        if hits.is_empty() {
            return ToolResult::ok("no matches");
        }
        let mut out = hits.join("\n");
        if hits.len() >= limit {
            out.push_str(&format!("\n[showing first {limit} matches]"));
        }
        ToolResult::ok(out)
    }
}

// ---------------------------------------------------------------------------
// glob
// ---------------------------------------------------------------------------

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files by glob pattern (e.g. '**/*.rs', 'src/**/*.ts'), sorted by \
         modification time (newest first)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern" },
                "path": { "type": "string", "description": "Directory to search (default: cwd)" }
            },
            "required": ["pattern"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Search
    }
    fn title(&self, args: &Value) -> String {
        format!("Find {}", short_target(args, &["pattern"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: pattern");
        };
        let base = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve(ctx, p))
            .unwrap_or_else(|| ctx.cwd.clone());
        let glob = match globset::Glob::new(pattern) {
            Ok(g) => g.compile_matcher(),
            Err(e) => return ToolResult::err(format!("invalid glob: {e}")),
        };
        let mut matches: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        let walker = ignore::WalkBuilder::new(&base)
            .hidden(false)
            .git_ignore(ctx.config.tools.respect_gitignore)
            .build();
        for entry in walker.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let rel = path.strip_prefix(&base).unwrap_or(path);
            if glob.is_match(rel) || glob.is_match(path) {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                matches.push((path.to_path_buf(), mtime));
            }
            if matches.len() > 1000 {
                break;
            }
        }
        matches.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        if matches.is_empty() {
            return ToolResult::ok("no files matched");
        }
        let out = matches
            .into_iter()
            .take(300)
            .map(|(p, _)| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        ToolResult::ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolContext};

    fn context(cwd: &std::path::Path) -> ToolContext {
        ToolContext::new(
            cwd.to_path_buf(),
            std::sync::Arc::new(crate::config::Config::default()),
            std::sync::Arc::new(crate::mcp::McpManager::new(Default::default(), "test")),
        )
    }

    fn sandbox(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cachy-fs-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every tool gets nonsense and has to answer with an error, never a panic.
    #[tokio::test]
    async fn hostile_inputs_do_not_panic() {
        let dir = sandbox("hostile");
        let ctx = context(&dir);
        let missing = dir.join("nao-existe.txt");
        let missing = missing.to_string_lossy().to_string();

        // read_file: missing, a directory, a binary file, and offsets past EOF.
        let binary = dir.join("bin.dat");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x01, 0x80, 0x81]).unwrap();
        let text = dir.join("texto.txt");
        std::fs::write(&text, "café ☕\n日本語\n").unwrap();
        for path in [
            missing.clone(),
            dir.to_string_lossy().to_string(),
            binary.to_string_lossy().to_string(),
        ] {
            let result = ReadFileTool
                .run(serde_json::json!({ "file_path": path }), &ctx)
                .await;
            assert!(
                result.is_error,
                "read_file should refuse {path}: {:?}",
                result.content
            );
        }
        for offset in [0i64, 1, 99, -3] {
            let result = ReadFileTool
                .run(
                    serde_json::json!({ "file_path": text.to_string_lossy(), "offset": offset, "limit": 2 }),
                    &ctx,
                )
                .await;
            let _ = result;
        }

        // search_replace: missing file, and an old_string that is not there.
        let result = SearchReplaceTool
            .run(
                serde_json::json!({ "file_path": missing, "old_string": "a", "new_string": "b" }),
                &ctx,
            )
            .await;
        assert!(result.is_error);
        let result = SearchReplaceTool
            .run(
                serde_json::json!({
                    "file_path": text.to_string_lossy(),
                    "old_string": "não está aqui",
                    "new_string": "x"
                }),
                &ctx,
            )
            .await;
        assert!(
            result.is_error,
            "missing old_string is an error, not a panic"
        );

        // grep: an invalid regex, and a path that does not exist.
        for args in [
            serde_json::json!({ "pattern": "([unclosed" }),
            serde_json::json!({ "pattern": "x", "path": missing }),
            serde_json::json!({ "pattern": "*nao-e-regex*" }),
        ] {
            let result = GrepTool.run(args.clone(), &ctx).await;
            let _ = result;
        }

        // glob: a broken pattern and a broken path.
        for args in [
            serde_json::json!({ "pattern": "[" }),
            serde_json::json!({ "pattern": "**/*", "path": missing }),
            serde_json::json!({ "pattern": "" }),
        ] {
            let result = GlobTool.run(args.clone(), &ctx).await;
            let _ = result;
        }

        // list_dir: missing, and a file instead of a directory.
        for path in [missing.clone(), text.to_string_lossy().to_string()] {
            let result = ListDirTool
                .run(serde_json::json!({ "path": path }), &ctx)
                .await;
            assert!(result.is_error, "list_dir should refuse {path}");
        }

        // write: a path inside a missing directory, and an empty path.
        for path in [
            dir.join("a/b/c/out.txt").to_string_lossy().to_string(),
            String::new(),
        ] {
            let result = WriteTool
                .run(
                    serde_json::json!({ "file_path": path, "content": "x" }),
                    &ctx,
                )
                .await;
            let _ = result;
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Arguments the model can invent must be refused, not panicked on.
    #[tokio::test]
    async fn wrong_argument_types_are_errors() {
        let dir = sandbox("types");
        let ctx = context(&dir);
        for args in [
            serde_json::json!({}),
            serde_json::json!({ "file_path": 12 }),
            serde_json::json!({ "file_path": null }),
            serde_json::json!({ "file_path": ["a"] }),
            serde_json::json!("not an object"),
        ] {
            let result = ReadFileTool.run(args.clone(), &ctx).await;
            assert!(result.is_error, "read_file accepted {args}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
