//! JUnit-style XML results (what CI servers read).

use std::fmt::Write;

use super::{Status, Summary, TestCase, TestResult};

/// `<testsuites>` with one `<testsuite>` per test file, in run order.
pub fn write(results: &[(TestCase, TestResult)]) -> String {
    let summary = Summary::of(results.iter().map(|(_, r)| r));
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<testsuites name=\"Misty Tests\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">",
        results.len(),
        summary.failed,
        summary.skipped,
        summary.duration
    );
    let mut files: Vec<&str> = Vec::new();
    for (c, _) in results {
        if !files.contains(&c.file.as_str()) {
            files.push(&c.file);
        }
    }
    for file in files {
        let cases: Vec<&(TestCase, TestResult)> =
            results.iter().filter(|(c, _)| c.file == file).collect();
        let s = Summary::of(cases.iter().map(|(_, r)| r));
        let _ = writeln!(
            out,
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">",
            escape(file),
            cases.len(),
            s.failed,
            s.skipped,
            s.duration
        );
        for (c, r) in cases {
            let class = classname(c);
            let _ = write!(
                out,
                "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\" file=\"{}\" line=\"{}\"",
                escape(&class),
                escape(&c.name),
                r.duration,
                escape(&c.file),
                c.line
            );
            let body = case_body(r);
            if body.is_empty() {
                out.push_str("/>\n");
            } else {
                out.push_str(">\n");
                out.push_str(&body);
                out.push_str("    </testcase>\n");
            }
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

fn case_body(r: &TestResult) -> String {
    let mut body = String::new();
    match r.status {
        Status::Failed => {
            let f = r.failure.clone().unwrap_or_default();
            let _ = writeln!(
                body,
                "      <failure message=\"{}\" type=\"failure\">{}</failure>",
                escape(&f.full_message()),
                escape(format!("{}\n{}", f.full_message(), f.traceback).trim_end())
            );
        }
        Status::Skipped => body.push_str("      <skipped/>\n"),
        _ => {}
    }
    if !r.logs.is_empty() {
        let _ = writeln!(
            body,
            "      <system-out>{}</system-out>",
            escape(&r.logs.join("\n"))
        );
    }
    body
}

/// `Tests.MathTests.Suite.Inner`: the file (without `Assets/` and the test suffix) and suites.
fn classname(c: &TestCase) -> String {
    let file = c
        .file
        .trim_start_matches("Assets/")
        .trim_end_matches(super::PLAY_SUFFIX)
        .trim_end_matches(super::EDIT_SUFFIX)
        .replace('/', ".");
    std::iter::once(file)
        .chain(c.suites.iter().cloned())
        .collect::<Vec<_>>()
        .join(".")
}

/// XML text/attribute escaping (control characters other than tab/newline are dropped).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' | '\t' | '\r' => out.push(ch),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}
