//! Run reporters: JSON, JUnit XML (Newman-like, for CI) and a self-contained
//! HTML page.

use std::fmt::Write as _;

use serde::Serialize;

use crate::run::{RequestExtra, RequestRunResult, RunReport, RunStats, RunSummary};

#[derive(Serialize)]
struct JsonResult<'a> {
    #[serde(flatten)]
    result: &'a RequestRunResult,
    #[serde(flatten)]
    extra: &'a RequestExtra,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    tool: &'static str,
    version: &'static str,
    summary: &'a RunSummary,
    stats: &'a RunStats,
    results: Vec<JsonResult<'a>>,
    environment_name: &'a Option<String>,
    environment: &'a indexmap::IndexMap<String, String>,
    globals: &'a indexmap::IndexMap<String, String>,
}

/// Full results as pretty JSON.
pub fn json_report(report: &RunReport) -> String {
    let r = JsonReport {
        tool: "ilixpatch",
        version: env!("CARGO_PKG_VERSION"),
        summary: &report.summary,
        stats: &report.stats,
        results: report
            .results
            .iter()
            .map(|result| JsonResult {
                result,
                extra: &result.extra,
            })
            .collect(),
        environment_name: &report.environment_name,
        environment: &report.environment,
        globals: &report.globals,
    };
    serde_json::to_string_pretty(&r).expect("report is serializable")
}

/// Escape text for XML/HTML attributes and content.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn secs(ms: f64) -> String {
    format!("{:.3}", ms / 1000.0)
}

fn suite_name(r: &RequestRunResult, multi_iter: bool) -> String {
    let base = if r.extra.item_path.is_empty() {
        r.name.clone()
    } else {
        r.extra.item_path.clone()
    };
    if multi_iter {
        format!("{base} (iteration {})", r.iteration + 1)
    } else {
        base
    }
}

/// JUnit XML: one `<testsuite>` per executed request, one `<testcase>` per
/// `pm.test`. Request/script errors are reported as `<error>` testcases.
pub fn junit_report(report: &RunReport, name: &str) -> String {
    let multi_iter = report.summary.iterations > 1;
    let mut suites = String::new();
    let (mut total_tests, mut total_failures, mut total_errors) = (0usize, 0usize, 0usize);
    for (id, r) in report.results.iter().enumerate() {
        let classname = escape(&format!(
            "{}.{}",
            r.extra.collection,
            r.extra.item_path.replace('/', ".")
        ));
        let time = secs(r.elapsed_ms);
        let mut cases = String::new();
        let mut failures = 0;
        let mut errors = 0;
        let mut skipped = 0;
        for t in &r.tests {
            let _ = write!(
                cases,
                "      <testcase name=\"{}\" time=\"{time}\" classname=\"{classname}\">",
                escape(&t.name)
            );
            if t.skipped {
                skipped += 1;
                cases.push_str("<skipped/>");
            } else if !t.passed {
                failures += 1;
                let msg = t.error.as_deref().unwrap_or("assertion failed");
                let _ = write!(
                    cases,
                    "\n        <failure type=\"AssertionFailure\" message=\"{}\"><![CDATA[{}]]></failure>\n      ",
                    escape(msg),
                    msg.replace("]]>", "]]]]><![CDATA[>")
                );
            }
            cases.push_str("</testcase>\n");
        }
        if let Some(e) = &r.error {
            errors += 1;
            let kind = if r.extra.request_failed {
                "RequestError"
            } else {
                "ScriptError"
            };
            let _ = write!(
                cases,
                "      <testcase name=\"{}\" time=\"{time}\" classname=\"{classname}\">\n        <error type=\"{kind}\" message=\"{}\"><![CDATA[{}]]></error>\n      </testcase>\n",
                escape(&format!("{} {}", r.method.as_str(), r.url)),
                escape(e),
                e.replace("]]>", "]]]]><![CDATA[>")
            );
        }
        let tests = r.tests.len() + errors;
        total_tests += tests;
        total_failures += failures;
        total_errors += errors;
        let _ = write!(
            suites,
            "  <testsuite name=\"{}\" id=\"{id}\" tests=\"{tests}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time}\">\n{cases}  </testsuite>\n",
            escape(&suite_name(r, multi_iter)),
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"{}\" tests=\"{total_tests}\" failures=\"{total_failures}\" errors=\"{total_errors}\" time=\"{}\">\n{suites}</testsuites>\n",
        escape(name),
        secs(report.summary.total_ms),
    )
}

const HTML_CSS: &str = r#"
:root{--bg:#f7f7f8;--surface:#fff;--text:#1d1d1f;--muted:#6b6b73;--border:#e2e2e6;--ok:#1a7f37;--bad:#cf222e;--warn:#9a6700;--accent:#5b5bd6}
@media (prefers-color-scheme:dark){:root{--bg:#141416;--surface:#1d1d20;--text:#ececf1;--muted:#9a9aa3;--border:#2e2e33;--ok:#3fb950;--bad:#f85149;--warn:#d29922;--accent:#8b8bf5}}
*{box-sizing:border-box}body{margin:0;font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:var(--bg);color:var(--text)}
main{max-width:1100px;margin:0 auto;padding:24px 16px}h1{font-size:22px;margin:0 0 4px}.sub{color:var(--muted);margin-bottom:20px}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin-bottom:24px}
.card{background:var(--surface);border:1px solid var(--border);border-radius:10px;padding:12px 14px}.card b{display:block;font-size:22px}.card span{color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.04em}
.req{background:var(--surface);border:1px solid var(--border);border-radius:10px;margin-bottom:10px;overflow:hidden}
.req summary{cursor:pointer;padding:10px 14px;display:flex;gap:10px;align-items:center;flex-wrap:wrap}
.m{font-weight:700;font-family:ui-monospace,Menlo,monospace;font-size:12px;color:var(--accent);min-width:56px}
.path{font-weight:600}.url{color:var(--muted);font-family:ui-monospace,Menlo,monospace;font-size:12px;word-break:break-all;flex:1 1 300px}
.st{font-family:ui-monospace,Menlo,monospace;font-size:12px}.ok{color:var(--ok)}.bad{color:var(--bad)}.skip{color:var(--warn)}
.body{padding:4px 14px 12px;border-top:1px solid var(--border)}ul{list-style:none;padding:0;margin:8px 0}li{padding:2px 0}
.err{color:var(--bad);white-space:pre-wrap;font-family:ui-monospace,Menlo,monospace;font-size:12px}
pre{background:var(--bg);padding:8px;border-radius:6px;overflow:auto;font-size:12px;max-height:300px}
"#;

/// Self-contained HTML report (inline CSS, no external assets).
pub fn html_report(report: &RunReport, name: &str) -> String {
    let s = &report.summary;
    let st = &report.stats;
    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} · ilixPatch report</title><style>{HTML_CSS}</style></head><body><main>\n<h1>{}</h1><div class=\"sub\">ilixPatch {} · {} iteration(s) · {:.0} ms total{}</div>\n",
        escape(name),
        escape(name),
        env!("CARGO_PKG_VERSION"),
        s.iterations,
        s.total_ms,
        if s.cancelled { " · cancelled" } else { "" }
    );
    let card = |h: &mut String, label: &str, value: String, bad: bool| {
        let _ = write!(
            h,
            "<div class=\"card\"><span>{label}</span><b class=\"{}\">{value}</b></div>",
            if bad { "bad" } else { "" }
        );
    };
    h.push_str("<div class=\"cards\">");
    card(&mut h, "Requests", s.requests.to_string(), false);
    card(
        &mut h,
        "Failed requests",
        s.failed_requests.to_string(),
        s.failed_requests > 0,
    );
    card(&mut h, "Assertions", s.tests.to_string(), false);
    card(
        &mut h,
        "Failed assertions",
        s.failed_tests.to_string(),
        s.failed_tests > 0,
    );
    card(
        &mut h,
        "Script errors",
        (st.failed_prerequest_scripts + st.failed_test_scripts).to_string(),
        st.failed_prerequest_scripts + st.failed_test_scripts > 0,
    );
    card(
        &mut h,
        "Avg response",
        format!("{:.0} ms", s.avg_response_ms),
        false,
    );
    h.push_str("</div>\n");

    let multi_iter = s.iterations > 1;
    for r in &report.results {
        let failed = r.failed();
        let status = match (r.status, &r.status_text) {
            _ if r.extra.skipped => "<span class=\"skip\">skipped</span>".to_string(),
            (Some(code), text) => format!(
                "<span class=\"{}\">{code} {}</span>",
                if (200..400).contains(&code) {
                    "ok"
                } else {
                    "bad"
                },
                escape(text.as_deref().unwrap_or(""))
            ),
            (None, _) => "<span class=\"bad\">error</span>".to_string(),
        };
        let _ = write!(
            h,
            "<details class=\"req\"{}><summary><span class=\"{}\">{}</span><span class=\"m\">{}</span><span class=\"path\">{}</span><span class=\"url\">{}</span><span class=\"st\">{status} · {:.0} ms · {} B</span></summary><div class=\"body\">",
            if failed { " open" } else { "" },
            if failed { "bad" } else { "ok" },
            if failed { "✗" } else { "✓" },
            r.method.as_str(),
            escape(&suite_name(r, multi_iter)),
            escape(&r.url),
            r.elapsed_ms,
            r.size_bytes,
        );
        if let Some(e) = &r.error {
            let _ = write!(h, "<div class=\"err\">{}</div>", escape(e));
        }
        if !r.tests.is_empty() {
            h.push_str("<ul>");
            for t in &r.tests {
                let (cls, mark) = if t.skipped {
                    ("skip", "–")
                } else if t.passed {
                    ("ok", "✓")
                } else {
                    ("bad", "✗")
                };
                let _ = write!(
                    h,
                    "<li class=\"{cls}\">{mark} {}{}</li>",
                    escape(&t.name),
                    t.error
                        .as_deref()
                        .map(|e| format!("<div class=\"err\">{}</div>", escape(e)))
                        .unwrap_or_default()
                );
            }
            h.push_str("</ul>");
        } else if r.error.is_none() {
            h.push_str("<div class=\"sub\">No tests.</div>");
        }
        if !r.logs.is_empty() {
            let _ = write!(h, "<pre>{}</pre>", escape(&r.logs.join("\n")));
        }
        if let Some(body) = &r.extra.response_body {
            let shown: String = body.chars().take(20_000).collect();
            let _ = write!(h, "<pre>{}</pre>", escape(&shown));
        }
        h.push_str("</div></details>\n");
    }
    h.push_str("</main></body></html>\n");
    h
}
