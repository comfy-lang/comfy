//! Source spans and diagnostic reporting.
//!
//! Every user-facing error in the compiler should be reported as a
//! `Diagnostic` with a `Span`, rather than a `panic!`. Panics are reserved
//! for internal compiler bugs (invariant violations), never for invalid
//! user input.

/// A byte-offset range into the original source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Merge two spans into the smallest span containing both.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self { message: message.into(), span }
    }

    /// Render this diagnostic as a human-readable, pointer-annotated message,
    /// e.g.:
    ///
    /// ```text
    /// error: expected ')', found ';'
    ///   --> main.cfy:3:18
    ///    |
    ///  3 |     $syscall(1, 2;
    ///    |                  ^
    /// ```
    pub fn render(&self, file: &str, source: &str) -> String {
        let (line, col, line_text) = locate(source, self.span.start);

        let gutter = format!("{}", line);
        let pad = " ".repeat(gutter.len());
        let caret_offset = col.saturating_sub(1);

        format!(
            "error: {}\n{}--> {}:{}:{}\n{} |\n{} | {}\n{} | {}^",
            self.message,
            pad,
            file,
            line,
            col,
            pad,
            gutter,
            line_text,
            pad,
            " ".repeat(caret_offset),
        )
    }
}

/// Given a byte offset, find the (1-based line, 1-based column, line text).
fn locate(source: &str, offset: usize) -> (usize, usize, &str) {
    let offset = offset.min(source.len());
    let mut line_start = 0;
    let mut line_no = 1;

    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line_no += 1;
            line_start = i + 1;
        }
    }

    let line_end = source[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(source.len());

    let col = source[line_start..offset].chars().count() + 1;
    (line_no, col, &source[line_start..line_end])
}
