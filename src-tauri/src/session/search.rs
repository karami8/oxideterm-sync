// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Terminal search engine for scroll buffer
//!
//! Provides regex-based search with:
//! - Case-sensitive/insensitive matching
//! - Whole word matching
//! - Multi-threaded execution (spawn_blocking)
//! - Performance optimized for large buffers

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::scroll_buffer::TerminalLine;

/// Search options for terminal content
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchOptions {
    /// Search query
    pub query: String,
    /// Case-sensitive matching
    pub case_sensitive: bool,
    /// Use regex (if false, treat as literal string)
    pub regex: bool,
    /// Match whole words only
    pub whole_word: bool,
    /// Maximum matches to return (0 = unlimited, default 1000)
    #[serde(default = "default_max_matches")]
    pub max_matches: usize,
}

fn default_max_matches() -> usize {
    100
}

/// Single search match result
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    /// Line number in buffer (0-indexed)
    pub line_number: usize,
    /// Column start position (0-indexed)
    pub column_start: usize,
    /// Column end position (0-indexed)
    pub column_end: usize,
    /// Matched text
    pub matched_text: String,
    /// Full line content for context
    pub line_content: String,
}

/// Search result with all matches and metadata
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// All matches found (up to max_matches)
    pub matches: Vec<SearchMatch>,
    /// Total number of matches
    pub total_matches: usize,
    /// Search duration in milliseconds
    pub duration_ms: u64,
    /// Whether results were truncated due to max_matches limit
    #[serde(default)]
    pub truncated: bool,
    /// Error message if regex is invalid (None = no error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Execute search on terminal lines
pub fn search_lines(lines: &[TerminalLine], options: SearchOptions) -> SearchResult {
    let start = Instant::now();
    let mut matches = Vec::new();

    // Build regex pattern
    let pattern = if options.regex {
        // User provided regex
        options.query.clone()
    } else {
        // Escape special regex characters for literal search
        let escaped = regex::escape(&options.query);
        if options.whole_word {
            format!(r"\b{}\b", escaped)
        } else {
            escaped
        }
    };

    // Build regex with appropriate flags
    let regex = match RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .build()
    {
        Ok(re) => re,
        Err(e) => {
            // Invalid regex, return error with message
            return SearchResult {
                matches: vec![],
                total_matches: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                truncated: false,
                error: Some(format!("Invalid regex: {}", e)),
            };
        }
    };

    // Effective limit: 0 means unlimited
    let limit = if options.max_matches == 0 {
        usize::MAX
    } else {
        options.max_matches
    };
    let mut total_matches: usize = 0;
    let capped = limit < usize::MAX;

    // Search through all lines
    for (_line_number, line) in lines.iter().enumerate() {
        // Find all matches in this line; share line_content across matches on the same line
        let mut line_content_cached: Option<String> = None;
        for cap in regex.find_iter(&line.text) {
            total_matches += 1;
            if matches.len() < limit {
                let line_content = match &line_content_cached {
                    Some(c) => c.clone(),
                    None => {
                        let c = line.text.clone();
                        line_content_cached = Some(c.clone());
                        c
                    }
                };
                matches.push(SearchMatch {
                    line_number: _line_number,
                    column_start: cap.start(),
                    column_end: cap.end(),
                    matched_text: cap.as_str().to_string(),
                    line_content,
                });
            } else if !capped {
                // Unlimited mode: should not happen, but break defensively
                break;
            }
            // When capped, keep counting but skip pushing
        }
    }

    let truncated = total_matches > matches.len();
    let duration_ms = start.elapsed().as_millis() as u64;

    SearchResult {
        total_matches,
        matches,
        duration_ms,
        truncated,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_line(text: &str) -> TerminalLine {
        TerminalLine::new(text.to_string())
    }

    #[test]
    fn test_literal_search() {
        let lines = vec![
            make_line("Hello world"),
            make_line("Hello Rust"),
            make_line("Goodbye world"),
        ];

        let options = SearchOptions {
            query: "Hello".to_string(),
            case_sensitive: true,
            regex: false,
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.matches[0].line_number, 0);
        assert_eq!(result.matches[1].line_number, 1);
    }

    #[test]
    fn test_case_insensitive() {
        let lines = vec![
            make_line("Hello World"),
            make_line("hello world"),
            make_line("HELLO WORLD"),
        ];

        let options = SearchOptions {
            query: "hello".to_string(),
            case_sensitive: false,
            regex: false,
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 3);
    }

    #[test]
    fn test_whole_word() {
        let lines = vec![
            make_line("hello world"),
            make_line("helloworld"),
            make_line("say hello please"),
        ];

        let options = SearchOptions {
            query: "hello".to_string(),
            case_sensitive: false,
            regex: false,
            whole_word: true,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 2); // Matches line 0 and 2, not 1
        assert_eq!(result.matches[0].line_number, 0);
        assert_eq!(result.matches[1].line_number, 2);
    }

    #[test]
    fn test_regex_search() {
        let lines = vec![
            make_line("Error: file not found"),
            make_line("Warning: deprecated API"),
            make_line("Info: starting server"),
            make_line("Error: connection timeout"),
        ];

        let options = SearchOptions {
            query: r"^Error:".to_string(),
            case_sensitive: true,
            regex: true,
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.matches[0].line_number, 0);
        assert_eq!(result.matches[1].line_number, 3);
    }

    #[test]
    fn test_multiple_matches_per_line() {
        let lines = vec![make_line("test test test")];

        let options = SearchOptions {
            query: "test".to_string(),
            case_sensitive: true,
            regex: false,
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 3);
    }

    #[test]
    fn test_no_matches() {
        let lines = vec![make_line("Hello world"), make_line("Goodbye world")];

        let options = SearchOptions {
            query: "Rust".to_string(),
            case_sensitive: true,
            regex: false,
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 0);
        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_invalid_regex() {
        let lines = vec![make_line("test")];

        let options = SearchOptions {
            query: "[invalid(".to_string(),
            case_sensitive: true,
            regex: true,
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 0); // Should not panic, return empty
        assert!(result.error.is_some()); // Should have error message
        assert!(result.error.unwrap().contains("Invalid regex"));
    }

    #[test]
    fn test_special_characters_literal() {
        let lines = vec![make_line("file.txt"), make_line("fileXtxt")];

        let options = SearchOptions {
            query: "file.txt".to_string(),
            case_sensitive: true,
            regex: false, // Literal search, dot should be escaped
            whole_word: false,
            max_matches: 0,
        };

        let result = search_lines(&lines, options);
        assert_eq!(result.total_matches, 1); // Only matches exact "file.txt"
        assert_eq!(result.matches[0].line_number, 0);
    }
}
