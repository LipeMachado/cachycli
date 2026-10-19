//! Fuzzy file indexing for the `@` reference picker and the `/` slash menu.

use std::path::PathBuf;

/// A flat index of files under a root, refreshed on demand.
pub struct FileIndex {
    root: PathBuf,
    files: Vec<String>,
}

impl FileIndex {
    pub fn new(root: PathBuf) -> Self {
        let mut index = Self {
            root,
            files: Vec::new(),
        };
        index.refresh();
        index
    }

    pub fn refresh(&mut self) {
        let mut files = Vec::new();
        let walker = ignore::WalkBuilder::new(&self.root)
            .hidden(true)
            .git_ignore(true)
            .max_depth(Some(12))
            .build();
        for entry in walker.flatten() {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if files.len() > 20_000 {
                break;
            }
            if let Ok(rel) = path.strip_prefix(&self.root) {
                files.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        files.sort();
        self.files = files;
    }

    /// Fuzzy match the query; returns the best candidates, dirs first.
    pub fn search(&self, query: &str, include_hidden: bool) -> Vec<String> {
        let query = query.trim();
        if query.is_empty() {
            return self
                .files
                .iter()
                .filter(|f| include_hidden || !f.split('/').any(|p| p.starts_with('.')))
                .take(200)
                .cloned()
                .collect();
        }
        let mut scored: Vec<(i64, &String)> = Vec::new();
        for file in &self.files {
            if !include_hidden && file.split('/').any(|p| p.starts_with('.')) {
                continue;
            }
            if let Some(score) = fuzzy_score(query, file) {
                scored.push((score, file));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
        scored
            .into_iter()
            .take(200)
            .map(|(_, f)| f.clone())
            .collect()
    }

    /// Number of files in the index.
    pub fn len(&self) -> usize {
        self.files.len()
    }
}

/// Subsequence fuzzy score: higher is better; `None` when not a match.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let query_lower = query.to_lowercase();
    let candidate_lower = candidate.to_lowercase();
    if query_lower.contains('/') {
        // Path-aware: require the substring in order per component.
        let mut score = 0i64;
        let mut rest = candidate_lower.as_str();
        for part in query_lower.split('/') {
            let pos = rest.find(part)?;
            // Prefer matches at component boundaries.
            let boundary_bonus =
                if pos == 0 || rest.as_bytes().get(pos.wrapping_sub(1)) == Some(&b'/') {
                    12
                } else {
                    0
                };
            score += 20 + boundary_bonus - (pos.min(10) as i64);
            let advance = pos + part.len();
            rest = &rest[advance..];
        }
        // Bonus when the whole query is a substring.
        if candidate_lower.contains(&query_lower) {
            score += 50;
        }
        return Some(score);
    }
    // Simple subsequence scoring with boundary bonuses.
    let mut score = 0i64;
    let mut chars = candidate_lower.char_indices().peekable();
    let mut last_match: Option<usize> = None;
    let mut matched = 0usize;
    for qc in query_lower.chars() {
        let mut found = false;
        for (idx, cc) in chars.by_ref() {
            if cc == qc {
                matched += 1;
                let boundary = idx == 0
                    || matches!(
                        candidate_lower.as_bytes().get(idx.wrapping_sub(1)),
                        Some(b'/') | Some(b'_') | Some(b'-') | Some(b'.')
                    );
                score += 10 + if boundary { 15 } else { 0 };
                if let Some(last) = last_match {
                    if idx == last + 1 {
                        score += 8; // consecutive
                    }
                }
                last_match = Some(idx);
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }
    // Prefer shorter candidates and basename hits.
    score -= (candidate.len() as i64) / 8;
    if let Some(base) = candidate.rsplit('/').next() {
        if fuzzy_score_simple(&query_lower, &base.to_lowercase()) {
            score += 25;
        }
    }
    if matched == 0 {
        None
    } else {
        Some(score)
    }
}

fn fuzzy_score_simple(query: &str, candidate: &str) -> bool {
    let mut it = candidate.chars();
    query.chars().all(|qc| it.any(|cc| cc == qc))
}

/// Read a file range `path:10-50` references.
pub fn parse_line_range(query: &str) -> (String, Option<(usize, usize)>) {
    if let Some((path, range)) = query.rsplit_once(':') {
        let parts: Vec<&str> = range.split('-').collect();
        if parts.len() == 2 {
            if let (Ok(a), Ok(b)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                return (path.to_string(), Some((a, b)));
            }
        } else if let Ok(a) = range.parse::<usize>() {
            return (path.to_string(), Some((a, a)));
        }
    }
    (query.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches() {
        assert!(fuzzy_score("sm", "src/main.rs").is_some());
        assert!(fuzzy_score("mainrs", "src/main.rs").is_some());
        assert!(fuzzy_score("zzzz", "src/main.rs").is_none());
        assert!(fuzzy_score("src/ma", "src/main.rs").is_some());
    }

    #[test]
    fn parses_ranges() {
        assert_eq!(
            parse_line_range("src/main.rs:10-50"),
            ("src/main.rs".into(), Some((10, 50)))
        );
        assert_eq!(
            parse_line_range("src/main.rs"),
            ("src/main.rs".into(), None)
        );
    }
}
