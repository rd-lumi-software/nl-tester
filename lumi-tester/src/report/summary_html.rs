//! Formal "test summary document" HTML report - light theme, structured sections
//! (overall result / environment / test content / failure log / attachments),
//! meant for printing/sharing with stakeholders. This is a different document from
//! `html::generate` (the dark dashboard-style debugging report with embedded UI
//! inspector) - both are generated side by side.
//!
//! Every field rendered here comes from data the runner actually captured for this
//! session (`TestResults`). Fields lumi-tester does not yet capture anywhere
//! (firmware/bridge/HC versions, phone model, sensor snapshots) are rendered as
//! "Chưa thu thập" (not captured) rather than filled with placeholder values -
//! see the note in the PR/commit message: `StandardSessionReport`'s JSON generator
//! (`report::json`) currently fabricates several of these fields (fake sensor
//! readings, a hardcoded app version, a hardcoded "staging" environment), which
//! this renderer deliberately does not reproduce.

use super::html::{format_duration, html_escape, target_name_from_session_id};
use super::types::{
    HcReportMetadata, MultiTargetOperationReport, MultiTargetReportMetadata, TestResults,
};
use crate::runner::state::{CommandStatus, FlowStatus};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

/// `output_dir` is the `output/<device>/` root (same directory
/// `html::generate_sessions_dashboard` scans) - when given, the report gets a
/// "Run History" section listing every past session that ran the same target
/// (file/flow), so "I ran this file 5 times" is visible directly in the report
/// instead of only in the separate sessions dashboard. `None` (e.g. the
/// `lumi-tester report` subcommand, which only has a standalone JSON file, no
/// sessions directory to scan) just omits that section rather than erroring.
pub async fn generate(
    results: &TestResults,
    app_id: Option<&str>,
    platform: Option<&str>,
    title: Option<&str>,
    output_dir: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    // The directory the report file itself will live in - every local file link
    // in the report (evidence screenshots/logs, source YAML) is rewritten
    // relative to THIS, not to the cwd `lumi-tester run` happened to be invoked
    // from. Evidence paths are stored relative to that invocation cwd, but the
    // report lives nested under sessions/<id>/report/ - a browser (or a static
    // file server) resolves a relative href against the HTML file's own
    // location, not the original cwd, so a raw stored path 404s (and doubles up
    // the path when served through a server rooted above the invocation cwd).
    let report_dir = output
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let html = render_summary_html(results, app_id, platform, title, output_dir, &report_dir);

    if let Some(path) = output {
        std::fs::write(path, html)?;
        println!("Summary report saved to: {}", path.display());
    } else {
        println!("{}", html);
    }

    Ok(())
}

/// Rewrites `target` as a path relative to `from_dir`, so a report file can link
/// to it correctly no matter where the report itself is opened from. Falls back
/// to the raw `target` string if either side can't be canonicalized (e.g. the
/// file no longer exists, or `target` is already a URL) - a broken-but-present
/// link degrades better than a panic or an empty href.
fn relative_href(from_dir: &Path, target: &str) -> String {
    let (from_abs, target_abs) = match (from_dir.canonicalize(), Path::new(target).canonicalize()) {
        (Ok(f), Ok(t)) => (f, t),
        _ => return target.to_string(),
    };

    let from_components: Vec<_> = from_abs.components().collect();
    let target_components: Vec<_> = target_abs.components().collect();
    let common_len = from_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut rel = std::path::PathBuf::new();
    for _ in common_len..from_components.len() {
        rel.push("..");
    }
    for comp in &target_components[common_len..] {
        rel.push(comp.as_os_str());
    }

    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().replace('\\', "/")
    }
}

struct RunHistoryEntry {
    created_at: String,
    passed: u64,
    failed: u64,
    duration_ms: u64,
    is_current: bool,
}

/// Scans `output_dir/sessions/*/session.json` for past runs of the same target
/// (matched via `target_name_from_session_id`, the same grouping the sessions
/// dashboard uses), oldest first. Best-effort: any read/parse error for an
/// individual session just skips that one entry rather than failing the report.
fn collect_run_history(
    output_dir: &Path,
    current_session_id: &str,
    target_name: &str,
) -> Vec<RunHistoryEntry> {
    let sessions_dir = output_dir.join("sessions");
    let mut entries = Vec::new();

    let Ok(read_dir) = std::fs::read_dir(&sessions_dir) else {
        return entries;
    };

    for entry in read_dir.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().to_string();
        let Ok(content) = std::fs::read_to_string(path.join("session.json")) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let summary = val.get("summary");
        let total_flows = summary
            .and_then(|s| s.get("totalFlows").or_else(|| s.get("total_flows")))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        if target_name_from_session_id(&session_id, total_flows) != target_name {
            continue;
        }
        let passed = summary
            .and_then(|s| s.get("passed"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let failed = summary
            .and_then(|s| s.get("failed"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let duration_ms = summary
            .and_then(|s| {
                s.get("totalDurationMs")
                    .or_else(|| s.get("total_duration_ms"))
            })
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let created_at = val
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        entries.push(RunHistoryEntry {
            is_current: session_id == current_session_id,
            created_at,
            passed,
            failed,
            duration_ms,
        });
    }

    entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    entries
}

fn render_run_history(entries: &[RunHistoryEntry]) -> String {
    if entries.len() <= 1 {
        return String::new();
    }
    let total = entries.len();
    let mut rows = String::new();
    for (idx, e) in entries.iter().enumerate() {
        let run_no = idx + 1;
        let (status_text, status_class) = if e.failed == 0 {
            ("PASS", "status-pass")
        } else {
            ("FAIL", "status-fail")
        };
        let row_style = if e.is_current {
            " style=\"font-weight:700\""
        } else {
            ""
        };
        let current_tag = if e.is_current {
            " <span style=\"color:#0759b8;font-size:11px\">(phiên này)</span>"
        } else {
            ""
        };
        rows.push_str(&format!(
            r#"<tr{row_style}><td>Lần {run_no}/{total}{current_tag}</td><td>{time}</td><td><span class="status {status_class}">{status_text}</span></td><td>{passed} pass / {failed} fail</td><td>{dur}</td></tr>"#,
            row_style = row_style,
            run_no = run_no,
            total = total,
            current_tag = current_tag,
            time = html_escape(&e.created_at),
            status_class = status_class,
            status_text = status_text,
            passed = e.passed,
            failed = e.failed,
            dur = format_duration(e.duration_ms)
        ));
    }
    format!(
        r#"<h2>4. Lịch sử chạy</h2>
<p>File/flow này đã chạy <strong>{total}</strong> lần (tính cả phiên hiện tại).</p>
<div class="table-wrap"><table class="history-table"><colgroup><col><col><col><col><col></colgroup><thead><tr><th>Lần chạy</th><th>Thời gian</th><th>Kết quả</th><th>Testcase</th><th>Thời lượng</th></tr></thead><tbody>{rows}</tbody></table></div>
"#,
        total = total,
        rows = rows
    )
}

struct PieSegment {
    class: &'static str,
    color: &'static str,
    label: &'static str,
    value: f64,
}

pub fn render_summary_html(
    results: &TestResults,
    app_id: Option<&str>,
    platform: Option<&str>,
    title: Option<&str>,
    output_dir: Option<&Path>,
    report_dir: &Path,
) -> String {
    let summary = &results.summary;
    let duration_ms = summary.total_duration_ms.unwrap_or(0);

    // `results.generated_at` is NOT reliably RFC3339 - the main report path
    // (executor.rs::finish) formats it as "%Y-%m-%d %H:%M:%S" local time, while
    // `report::json::generate_standard_session_report` assumes RFC3339 and silently
    // falls back to `Utc::now()` (losing the real end time) whenever parsing that
    // format fails - which is always, for the main path. `parse_generated_at`
    // handles both formats so "Thời gian test" reflects the real session window
    // instead of reproducing that fallback bug.
    let end_dt = parse_generated_at(&results.generated_at);
    let start_dt = end_dt - chrono::Duration::milliseconds(duration_ms as i64);
    let end_time = end_dt.to_rfc3339();
    let start_time = start_dt.to_rfc3339();

    // Testcase-level (flow-level) pass/fail. BLOCKED/ERROR/SKIPPED aren't modeled
    // as distinct flow outcomes by the runner today (only Passed/Failed/
    // PartiallyPassed), so those columns are always 0 - kept in the layout for
    // fidelity with the reference format rather than dropped silently.
    let total_testcases = results.flows.len() as u32;
    let passed_testcases = results
        .flows
        .iter()
        .filter(|f| matches!(f.status, FlowStatus::Passed))
        .count() as u32;
    let failed_testcases = total_testcases.saturating_sub(passed_testcases);
    let overall_pass = failed_testcases == 0 && total_testcases > 0;

    let pie_segments = [
        PieSegment {
            class: "chart-pass",
            color: "#16a34a",
            label: "PASSED",
            value: passed_testcases as f64,
        },
        PieSegment {
            class: "chart-fail",
            color: "#dc2626",
            label: "FAILED",
            value: failed_testcases as f64,
        },
        PieSegment {
            class: "chart-blocked",
            color: "#f59e0b",
            label: "BLOCKED",
            value: 0.0,
        },
        PieSegment {
            class: "chart-error",
            color: "#7f1d1d",
            label: "ERROR",
            value: 0.0,
        },
        PieSegment {
            class: "chart-skip",
            color: "#94a3b8",
            label: "SKIPPED",
            value: 0.0,
        },
    ];
    let pie_total: f64 = pie_segments.iter().map(|s| s.value).sum();
    let (pie_gradient, pie_labels) = render_pie(&pie_segments, pie_total);
    let legend_html = render_pie_legend(&pie_segments, pie_total);

    // Run index per testcase name ("Lần i/N") for flows repeated within this session.
    let mut name_totals: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for f in &results.flows {
        *name_totals.entry(f.flow_name.clone()).or_insert(0) += 1;
    }
    let mut name_seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let run_labels: Vec<String> = results
        .flows
        .iter()
        .map(|f| {
            let seen = name_seen.entry(f.flow_name.clone()).or_insert(0);
            *seen += 1;
            format!(
                "Lần {}/{}",
                seen,
                name_totals.get(&f.flow_name).copied().unwrap_or(1)
            )
        })
        .collect();

    let max_flow_duration = results
        .flows
        .iter()
        .map(|f| f.total_duration_ms.unwrap_or(0))
        .max()
        .unwrap_or(0);
    let mut duration_rows = String::new();
    for (flow, run_label) in results.flows.iter().zip(run_labels.iter()) {
        let dur = flow.total_duration_ms.unwrap_or(0);
        let pct = if max_flow_duration > 0 {
            (dur as f64 / max_flow_duration as f64 * 100.0).clamp(2.0, 100.0)
        } else {
            2.0
        };
        let bar_class = if matches!(flow.status, FlowStatus::Passed) {
            "chart-pass"
        } else {
            "chart-fail"
        };
        duration_rows.push_str(&format!(
            r#"<div class="duration-row"><div class="duration-label">{name} <span>{run_label}</span></div><div class="duration-track"><div class="duration-bar {bar_class}" style="width:{pct:.2}%"></div></div><strong class="duration-value">{dur} ms</strong></div>"#,
            name = html_escape(&flow.flow_name),
            run_label = html_escape(run_label),
            bar_class = bar_class,
            pct = pct,
            dur = dur
        ));
    }

    // Environment: only rows the runner actually has a real value for. Fields
    // with no data source yet (firmware/app/bridge/server/HC version, device
    // model) are omitted entirely rather than shown as a "Chưa thu thập"
    // placeholder row - a row that will never have data on any run just adds
    // noise, and its absence is itself the honest signal that it isn't tracked.
    let mut env_rows = String::new();
    if let Some(app) = app_id.filter(|s| !s.is_empty()) {
        env_rows.push_str(&format!(
            r#"<tr><th>Ứng dụng (App ID)</th><td>{}</td></tr>"#,
            html_escape(app)
        ));
    }
    if let Some(os) = platform.filter(|s| !s.is_empty()) {
        env_rows.push_str(&format!(
            r#"<tr><th>Nền tảng</th><td>{}</td></tr>"#,
            html_escape(os)
        ));
    }
    if env_rows.is_empty() {
        env_rows = r#"<tr><td colspan="2" style="text-align:center;color:#64748b">Chưa có thông tin môi trường nào được thu thập cho phiên này</td></tr>"#.to_string();
    }

    // Test content table (section 3) + failure log rows (section 4), built together
    // since both walk the same command list. Rows are grouped by source YAML file
    // (flow_path) - the closest thing to a "feature group" this data model has -
    // with a separator/summary row per group, and carry data-status/data-search
    // attributes for the client-side filter toolbar.
    let mut test_rows = String::new();
    let mut failure_rows = String::new();
    let mut cumulative_ms: u64 = 0;
    let mut failed_names: Vec<String> = Vec::new();
    let mut total_retries: u32 = 0;
    let mut last_group: Option<&str> = None;

    // Pre-compute per-group pass/total so the group header can show "x/y đạt"
    // before iterating (group = flow_path).
    let mut group_stats: std::collections::HashMap<&str, (u32, u32)> =
        std::collections::HashMap::new();
    for flow in &results.flows {
        let entry = group_stats.entry(flow.flow_path.as_str()).or_insert((0, 0));
        entry.1 += 1;
        if matches!(flow.status, FlowStatus::Passed) {
            entry.0 += 1;
        }
    }

    for (idx, (flow, run_label)) in results.flows.iter().zip(run_labels.iter()).enumerate() {
        let tc_id = format!("TC-{:02}", idx + 1);
        let is_pass = matches!(flow.status, FlowStatus::Passed);
        let (status_text, status_class) = if is_pass {
            ("PASS", "status-pass")
        } else {
            ("FAIL", "status-fail")
        };
        let dur = flow.total_duration_ms.unwrap_or(0);
        if !is_pass {
            failed_names.push(flow.flow_name.clone());
        }
        total_retries += flow.commands.iter().map(|c| c.retry_count).sum::<u32>();

        if last_group != Some(flow.flow_path.as_str()) {
            let (gp, gt) = group_stats
                .get(flow.flow_path.as_str())
                .copied()
                .unwrap_or((0, 0));
            let basename = std::path::Path::new(&flow.flow_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&flow.flow_path);
            test_rows.push_str(&format!(
                r#"<tr class="group-row"><td colspan="9" title="{full}">📁 {name} — {gp}/{gt} đạt</td></tr>"#,
                full = html_escape(&flow.flow_path),
                name = html_escape(basename),
                gp = gp,
                gt = gt
            ));
            last_group = Some(flow.flow_path.as_str());
        }

        let first_error = flow.error.clone().or_else(|| {
            flow.commands.iter().find_map(|c| match &c.status {
                CommandStatus::Failed { error } => Some(error.clone()),
                _ => None,
            })
        });
        let reason_cell = first_error
            .clone()
            .map(|e| html_escape(&e))
            .unwrap_or_else(|| "-".to_string());

        let evidence_cell = flow
            .commands
            .iter()
            .find_map(|c| c.screenshot_path.as_ref())
            .map(|p| embed_image_thumb(p, report_dir))
            .unwrap_or_else(|| "-".to_string());
        let log_cell = flow
            .commands
            .iter()
            .find_map(|c| c.log_path.as_ref())
            .map(|p| short_path_link(p, report_dir))
            .unwrap_or_else(|| "-".to_string());

        test_rows.push_str(&format!(
            r#"<tr data-status="{status_class}" data-search="{search}"><td>{tc_id}</td><td>{name}</td><td>{run_label}</td><td><span class="status {status_class}">{status_text}</span></td><td>{dur}</td><td>{reason}</td><td>{evidence}</td><td>{log}</td></tr>"#,
            status_class = status_class,
            search = html_escape(&format!("{} {}", tc_id, flow.flow_name).to_lowercase()),
            tc_id = tc_id,
            name = html_escape(&flow.flow_name),
            run_label = html_escape(run_label),
            status_text = status_text,
            dur = dur,
            reason = reason_cell,
            evidence = evidence_cell,
            log = log_cell,
        ));

        // Walk this flow's commands to (a) build failure rows with an estimated
        // wall-clock timestamp derived from cumulative duration, and (b) advance
        // the running cumulative offset for the next flow's estimate.
        for cmd in &flow.commands {
            let cmd_dur = cmd.duration_ms.unwrap_or(0);
            if let CommandStatus::Failed { error } = &cmd.status {
                let est_time =
                    (start_dt + chrono::Duration::milliseconds(cumulative_ms as i64)).to_rfc3339();
                let image_cell = cmd
                    .screenshot_path
                    .as_ref()
                    .map(|p| embed_image_thumb(p, report_dir))
                    .unwrap_or_else(|| "-".to_string());
                let video_cell = flow
                    .video_path
                    .as_ref()
                    .map(|p| short_path_link(p, report_dir))
                    .unwrap_or_else(|| "-".to_string());
                failure_rows.push_str(&format!(
                    r#"<tr><td>{tc_id} / step_{idx}_{cname}</td><td>{time}</td><td>CMD_{idx}</td><td>{err}</td><td>{img}</td><td>{vid}</td></tr>"#,
                    tc_id = tc_id,
                    idx = cmd.index,
                    cname = html_escape(&cmd.command_name),
                    time = html_escape(&est_time),
                    err = html_escape(error),
                    img = image_cell,
                    vid = video_cell
                ));
            }
            cumulative_ms += cmd_dur;
        }
    }
    if failure_rows.is_empty() {
        failure_rows = r#"<tr><td colspan='6' style='text-align:center;color:#64748b'>Không có lỗi phát sinh trong phiên kiểm thử này</td></tr>"#.to_string();
    }

    // Quick, purely factual takeaways (no analytical narrative - every sentence is
    // a direct readout of a number already in this report, so there's nothing here
    // that could be "wrong" the way a written verdict could be).
    let mut findings: Vec<String> = Vec::new();
    if total_testcases > 0 {
        findings.push(format!(
            "{passed}/{total} testcase đạt ({pct:.0}%).",
            passed = passed_testcases,
            total = total_testcases,
            pct = passed_testcases as f64 / total_testcases as f64 * 100.0
        ));
    }
    if !failed_names.is_empty() {
        let shown: Vec<&str> = failed_names.iter().take(5).map(|s| s.as_str()).collect();
        let suffix = if failed_names.len() > 5 {
            format!(" và {} testcase khác", failed_names.len() - 5)
        } else {
            String::new()
        };
        findings.push(format!(
            "{} testcase thất bại: {}{}.",
            failed_names.len(),
            shown.join(", "),
            suffix
        ));
    }
    if total_retries > 0 {
        findings.push(format!(
            "Có {} lượt retry trong phiên - đáng xem lại độ ổn định của các bước liên quan.",
            total_retries
        ));
    }
    findings.push(format!(
        "Tổng thời gian chạy: {}.",
        format_duration(duration_ms)
    ));
    let findings_html = findings
        .iter()
        .map(|f| format!("<li>{}</li>", html_escape(f)))
        .collect::<Vec<_>>()
        .join("");

    // Attachments: the actual source YAML file(s) executed this session - the one
    // piece of "where did this come from" data that's unconditionally real.
    let mut seen_paths = std::collections::HashSet::new();
    let mut attachment_rows = String::new();
    let mut n = 0;
    for flow in &results.flows {
        if seen_paths.insert(flow.flow_path.clone()) {
            n += 1;
            attachment_rows.push_str(&format!(
                r#"<tr><td>{n}</td><td>{name} ({path})</td><td><a href="{href}">Mở file YAML</a></td></tr>"#,
                n = n,
                name = html_escape(&flow.flow_name),
                path = html_escape(&flow.flow_path),
                href = html_escape(&relative_href(report_dir, &flow.flow_path))
            ));
        }
    }
    if attachment_rows.is_empty() {
        attachment_rows = r#"<tr><td colspan='3' style='text-align:center;color:#64748b'>Không có tài liệu đính kèm</td></tr>"#.to_string();
    }

    let report_title = title
        .filter(|s| !s.is_empty())
        .map(html_escape)
        .unwrap_or_else(|| "Báo cáo kết quả test".to_string());

    let run_history_section = output_dir
        .map(|dir| {
            let target_name = target_name_from_session_id(&results.session_id, results.flows.len());
            let entries = collect_run_history(dir, &results.session_id, &target_name);
            render_run_history(&entries)
        })
        .unwrap_or_default();
    let multi_target_section = results
        .multi_target
        .as_ref()
        .map(render_multi_target_section)
        .unwrap_or_default();
    let (overall_class, overall_text) = if overall_pass {
        ("overall-pass", "PASS")
    } else {
        ("overall-fail", "FAIL")
    };

    format!(
        r##"<!doctype html><html lang="vi"><head><meta charset="utf-8">
<title>{report_title}</title><style>
*{{box-sizing:border-box}}body{{font-family:Segoe UI,Arial,sans-serif;background:#eef2f6;color:#182230;margin:0;padding:28px;line-height:1.5}}
main{{max-width:1480px;margin:auto;background:#fff;border:1px solid #dfe5ec;border-radius:8px;padding:32px;box-shadow:0 8px 24px rgba(20,35,55,.08)}}
h1{{font-size:28px;line-height:1.25;margin:0 0 10px;color:#101828}}h2{{font-size:19px;margin:32px 0 14px;padding-bottom:8px;border-bottom:2px solid #d9e2ec;color:#1d2939}}
p{{margin:8px 0 14px}}.cards{{display:grid;grid-template-columns:repeat(6,minmax(130px,1fr));gap:12px;margin:20px 0 8px}}
.overall-result{{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:5px 24px;align-items:center;border-radius:6px;padding:16px 20px;margin:12px 0 20px}}
.overall-pass{{border:1px solid #bbf7d0;border-left:6px solid #16a34a;background:#f0fdf4}}
.overall-fail{{border:1px solid #fecaca;border-left:6px solid #dc2626;background:#fffafa}}
.overall-label{{font-size:13px;font-weight:700;color:#475467}}.overall-value{{grid-column:2;grid-row:1/3;font-size:28px;line-height:1;font-weight:800}}
.overall-pass .overall-value{{color:#15803d}}.overall-fail .overall-value{{color:#b42318}}
.overall-time{{font-size:13px;color:#475467}}.overall-time strong{{color:#344054}}
.card{{background:#f8fafc;border:1px solid #d8e0e9;border-radius:6px;padding:14px 16px;min-height:88px}}
.card span,.card strong{{display:block}}.card span{{font-size:12px;font-weight:700;color:#526174}}.card strong{{font-size:25px;margin-top:7px;color:#101828}}
.table-wrap{{width:100%;overflow-x:auto;border:1px solid #d7dee8;border-radius:6px;background:#fff}}
table{{width:100%;border-collapse:collapse;table-layout:fixed;background:#fff;font-size:13px}}th,td{{border-right:1px solid #d7dee8;border-bottom:1px solid #d7dee8;padding:11px 12px;text-align:left;vertical-align:top;overflow-wrap:anywhere;word-break:normal}}
th:last-child,td:last-child{{border-right:0}}tbody tr:last-child td{{border-bottom:0}}th{{background:#edf2f7;color:#344054;font-size:12px;font-weight:700;line-height:1.35}}tbody tr:nth-child(even){{background:#fafbfd}}
a{{color:#0759b8;text-decoration:none;display:inline-block;max-width:100%;overflow-wrap:anywhere}}a:hover{{text-decoration:underline}}code{{font-family:Consolas,monospace;background:#f2f4f7;padding:2px 5px;border-radius:3px}}
hr{{border:0;border-top:1px solid #e3e8ef;margin:7px 0}}.status{{display:inline-block;min-width:66px;padding:3px 7px;border-radius:4px;text-align:center;font-size:11px;font-weight:700}}
.status-pass{{background:#dcfce7;color:#166534}}.status-fail,.status-error{{background:#fee2e2;color:#991b1b}}.status-blocked{{background:#ffedd5;color:#9a3412}}.status-skip{{background:#e5e7eb;color:#374151}}
.env-table col:first-child{{width:24%}}.env-table col:last-child{{width:76%}}
.test-table{{min-width:1320px}}.test-table .c-id{{width:13%}}.test-table .c-name{{width:15%}}.test-table .c-run{{width:8%}}.test-table .c-status{{width:8%}}.test-table .c-duration{{width:9%}}.test-table .c-reason{{width:16%}}.test-table .c-evidence{{width:17%}}.test-table .c-log{{width:14%}}
.test-table td:nth-child(3),.test-table td:nth-child(4),.test-table td:nth-child(5){{white-space:nowrap}}
.missing-artifact{{color:#b42318;font-size:12px}}
.failure-table{{min-width:1280px}}.failure-table .c-id{{width:13%}}.failure-table .c-time{{width:13%}}.failure-table .c-evidence{{width:15%}}.failure-table .c-log{{width:17%}}.failure-table .c-image{{width:21%}}.failure-table .c-video{{width:21%}}
.attachment-table col:nth-child(1){{width:6%}}.attachment-table col:nth-child(2){{width:44%}}.attachment-table col:nth-child(3){{width:50%}}
.history-table col:nth-child(1){{width:22%}}.history-table col:nth-child(2){{width:22%}}.history-table col:nth-child(3){{width:14%}}.history-table col:nth-child(4){{width:22%}}.history-table col:nth-child(5){{width:20%}}
.charts{{display:grid;grid-template-columns:minmax(0,.9fr) minmax(0,1.4fr);gap:16px;margin:18px 0 8px}}.chart-panel{{border:1px solid #d8e0e9;border-radius:6px;padding:18px;background:#fbfcfe}}
.chart-panel h3{{font-size:14px;margin:0 0 14px;color:#344054}}.pie-layout{{display:grid;grid-template-columns:190px minmax(0,1fr);align-items:center;gap:22px}}
.pie-chart{{position:relative;width:180px;aspect-ratio:1;border-radius:50%;box-shadow:inset 0 0 0 1px rgba(15,23,42,.08)}}.pie-label{{position:absolute;transform:translate(-50%,-50%);color:#fff;font-size:12px;font-weight:800;text-shadow:0 1px 2px rgba(0,0,0,.45);white-space:nowrap}}
.chart-pass{{background:#16a34a}}.chart-fail{{background:#dc2626}}.chart-error{{background:#7f1d1d}}.chart-blocked{{background:#f59e0b}}.chart-skip{{background:#94a3b8}}
.legend{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px 14px;margin-top:14px}}.legend-item{{display:grid;grid-template-columns:10px 1fr auto;align-items:center;gap:7px;font-size:12px}}.legend-swatch{{width:10px;height:10px;border-radius:2px}}
.pie-layout .legend{{grid-template-columns:1fr;margin-top:0;width:100%}}.pie-layout .legend-item strong{{white-space:nowrap}}
.duration-row{{display:grid;grid-template-columns:minmax(150px,1fr) minmax(180px,1.4fr) 78px;align-items:center;gap:10px;margin:9px 0}}.duration-label{{font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.duration-label span{{color:#667085}}.duration-track{{height:10px;border-radius:3px;background:#e8edf3;overflow:hidden}}.duration-bar{{height:100%;border-radius:3px}}.duration-value{{font-size:11px;text-align:right;white-space:nowrap}}
@media(max-width:1000px){{body{{padding:12px}}main{{padding:20px}}.cards{{grid-template-columns:repeat(3,1fr)}}.pie-layout{{grid-template-columns:1fr;justify-items:center;width:100%}}.legend{{width:100%}}}}
.title-row{{display:flex;justify-content:space-between;align-items:flex-start;gap:16px;flex-wrap:wrap}}
.print-btn{{background:#087d58;color:#fff;border:none;padding:9px 18px;border-radius:6px;font-weight:700;font-size:13px;cursor:pointer;white-space:nowrap}}
.print-btn:hover{{opacity:.9}}
.findings{{margin:14px 0 0;padding-left:20px}}
.findings li{{margin:4px 0;font-size:13.5px}}
.group-row td{{background:#eef2f6;font-weight:700;font-size:12.5px;color:#344054}}
.evidence-thumb{{width:72px;height:auto;border-radius:4px;border:1px solid #d7dee8;display:block}}
.toolbar{{display:flex;gap:10px;margin:14px 0;flex-wrap:wrap;align-items:center}}
.toolbar input,.toolbar select{{padding:7px 10px;border:1px solid #d7dee8;border-radius:6px;font-size:13px}}
.toolbar input{{flex:1;min-width:200px}}
@media(max-width:1000px){{.charts{{grid-template-columns:1fr}}}}@media print{{.toolbar,.print-btn{{display:none}}@page{{size:A4 landscape;margin:10mm}}body{{background:#fff;padding:0}}main{{max-width:none;border:0;box-shadow:none;padding:0}}.table-wrap{{overflow:visible}}table{{font-size:9px}}th,td{{padding:6px}}.test-table,.failure-table{{min-width:0}}a{{color:#000}}}}
</style></head><body><main>
<div class="title-row"><h1>{report_title}</h1><button class="print-btn" onclick="window.print()">🖨️ In / Xuất PDF</button></div>
<p>Phiên: <code>{session_id}</code></p>
<h2>1. Đánh giá kết quả test tổng hợp</h2>
<section class="overall-result {overall_class}"><span class="overall-label">Kết quả toàn phiên</span><strong class="overall-value">{overall_text}</strong><span class="overall-time"><strong>Thời gian test:</strong> {start_time} - {end_time}</span></section>
<ul class="findings">{findings_html}</ul>
<div class="cards"><div class="card"><span>Tổng testcase</span><strong>{total}</strong></div><div class="card"><span>PASSED</span><strong style="color:#16a34a">{passed}</strong></div><div class="card"><span>FAILED</span><strong style="color:#dc2626">{failed}</strong></div><div class="card"><span>BLOCKED</span><strong style="color:#f59e0b">0</strong></div><div class="card"><span>ERROR</span><strong style="color:#7f1d1d">0</strong></div><div class="card"><span>SKIPPED</span><strong style="color:#64748b">{skipped}</strong></div></div>
<div class="charts"><section class="chart-panel"><h3>Phân bố kết quả</h3><div class="pie-layout"><div class="pie-chart" style="background:{pie_gradient}" role="img" aria-label="Phân bố trạng thái testcase">{pie_labels}</div><div class="legend">{legend_html}</div></div></section>
<section class="chart-panel"><h3>Thời gian thực thi theo lần chạy</h3>{duration_rows}</section></div>
<h2>2. Môi trường test</h2>
<div class="table-wrap"><table class="env-table"><colgroup><col><col></colgroup><thead><tr><th>Nội dung</th><th>Chi tiết</th></tr></thead><tbody>{env_rows}</tbody></table></div>
{multi_target_section}
<h2>3. Nội dung kiểm tra</h2>
<div class="toolbar">
    <input type="text" id="tcSearch" placeholder="Tìm theo ID hoặc tên testcase..." onkeyup="filterTestRows()">
    <select id="tcStatus" onchange="filterTestRows()">
        <option value="all">Tất cả trạng thái</option>
        <option value="status-pass">Chỉ PASS</option>
        <option value="status-fail">Chỉ FAIL</option>
    </select>
    <span id="tcCount" style="font-size:12px;color:#667085"></span>
</div>
<div class="table-wrap"><table class="test-table"><colgroup><col class="c-id"><col class="c-name"><col class="c-run"><col class="c-status"><col class="c-duration"><col class="c-reason"><col class="c-evidence"><col class="c-log"></colgroup><thead><tr><th>ID testcase</th><th>Tên testcase</th><th>Số lần chạy</th><th>Kết quả</th><th>Thời gian test (duration_ms)</th><th>Lý do/Nguyên nhân</th><th>Evidence lỗi</th><th>Log lỗi</th></tr></thead>
<tbody id="tcBody">{test_rows}</tbody></table></div>
{run_history_section}<h2>5. Thông tin log khi lỗi</h2><div class="table-wrap"><table class="failure-table"><colgroup><col class="c-id"><col class="c-time"><col class="c-evidence"><col class="c-log"><col class="c-image"><col class="c-video"></colgroup><thead><tr><th>ID testcase</th><th>Thời điểm lỗi (ước tính)</th><th>Evidence ID</th><th>Thông tin log</th><th>Hình ảnh đính kèm</th><th>Video đính kèm</th></tr></thead><tbody>{failure_rows}</tbody></table></div>
<h2>6. Tài liệu đính kèm</h2><div class="table-wrap"><table class="attachment-table"><colgroup><col><col><col></colgroup><thead><tr><th>STT</th><th>Tài liệu</th><th>Liên kết</th></tr></thead><tbody>{attachment_rows}</tbody></table></div>
</main>
<script>
function filterTestRows(){{
    const q=(document.getElementById('tcSearch').value||'').toLowerCase();
    const st=document.getElementById('tcStatus').value;
    const rows=document.querySelectorAll('#tcBody tr:not(.group-row)');
    let shown=0, total=0;
    rows.forEach(r=>{{
        total++;
        const s=r.getAttribute('data-status');
        const text=r.getAttribute('data-search')||'';
        const match=(st==='all'||s===st)&&text.includes(q);
        r.style.display=match?'':'none';
        if(match) shown++;
    }});
    document.querySelectorAll('#tcBody tr.group-row').forEach(g=>{{
        let next=g.nextElementSibling, anyVisible=false;
        while(next && !next.classList.contains('group-row')){{
            if(next.style.display!=='none') anyVisible=true;
            next=next.nextElementSibling;
        }}
        g.style.display=anyVisible?'':'none';
    }});
    document.getElementById('tcCount').textContent='Hiển thị '+shown+'/'+total+' testcase';
}}
filterTestRows();
</script>
</body></html>"##,
        report_title = report_title,
        session_id = html_escape(&results.session_id),
        overall_class = overall_class,
        overall_text = overall_text,
        start_time = html_escape(&start_time),
        end_time = html_escape(&end_time),
        total = total_testcases,
        passed = passed_testcases,
        failed = failed_testcases,
        skipped = summary.skipped,
        findings_html = findings_html,
        pie_gradient = pie_gradient,
        pie_labels = pie_labels,
        legend_html = legend_html,
        duration_rows = duration_rows,
        env_rows = env_rows,
        test_rows = test_rows,
        run_history_section = run_history_section,
        multi_target_section = multi_target_section,
        failure_rows = failure_rows,
        attachment_rows = attachment_rows,
    )
}

fn render_multi_target_section(metadata: &MultiTargetReportMetadata) -> String {
    let adapters = if metadata.adapters.is_empty() {
        "Not captured".to_string()
    } else {
        metadata
            .adapters
            .iter()
            .map(|adapter| html_escape(adapter))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut operation_rows = String::new();
    for operation in &metadata.operations {
        operation_rows.push_str(&render_multi_target_operation_row(operation));
    }
    if operation_rows.is_empty() {
        operation_rows = "<tr><td colspan=\"9\" style=\"text-align:center;color:#64748b\">Not captured</td></tr>".to_string();
    }

    let hc_section = metadata
        .hc
        .as_ref()
        .map(render_hc_summary_section)
        .unwrap_or_default();

    format!(
        r#"<h2>Multi-target execution</h2>
<div class="table-wrap"><table class="env-table"><tbody>
<tr><th>Adapters</th><td>{adapters}</td></tr>
<tr><th>Repeat count</th><td>{repeat_count}</td></tr>
<tr><th>Operation count</th><td>{operation_count}</td></tr>
</tbody></table></div>
<h2>Operation details</h2>
<div class="table-wrap"><table class="test-table"><thead><tr><th>#</th><th>Repeat</th><th>Target</th><th>Command</th><th>Adapter</th><th>Status</th><th>Category</th><th>Message</th><th>Evidence</th></tr></thead><tbody>{operation_rows}</tbody></table></div>
{hc_section}"#,
        adapters = adapters,
        repeat_count = metadata.repeat_count,
        operation_count = metadata.operation_count,
        operation_rows = operation_rows,
        hc_section = hc_section,
    )
}

fn render_multi_target_operation_row(operation: &MultiTargetOperationReport) -> String {
    let status_class = match operation.status.as_str() {
        "passed" => "status-pass",
        "blocked" => "status-blocked",
        "error" => "status-error",
        _ => "status-fail",
    };
    let evidence = operation
        .normalized_evidence
        .as_ref()
        .map(compact_json)
        .unwrap_or_else(|| "Not captured".to_string());
    format!(
        r#"<tr><td>{sequence}</td><td>{repeat}</td><td>{target}</td><td><code>{command}</code></td><td>{adapter}</td><td><span class="status {status_class}">{status}</span></td><td>{category}</td><td>{message}</td><td><code>{evidence}</code></td></tr>"#,
        sequence = operation.sequence,
        repeat = operation.repeat_index + 1,
        target = html_escape(&operation.target),
        command = html_escape(&operation.command),
        adapter = html_escape(&operation.adapter),
        status_class = status_class,
        status = html_escape(&operation.status),
        category = html_escape(&operation.category),
        message = html_escape(&operation.message),
        evidence = html_escape(&evidence),
    )
}

fn render_hc_summary_section(metadata: &HcReportMetadata) -> String {
    let devices = if metadata.device_ids.is_empty() {
        "Not captured".to_string()
    } else {
        metadata
            .device_ids
            .iter()
            .map(|device| html_escape(device))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut rows = String::new();
    for readback in &metadata.readbacks {
        let values = readback
            .values
            .as_ref()
            .map(compact_json)
            .unwrap_or_else(|| "Not captured".to_string());
        rows.push_str(&format!(
            r#"<tr><td>{}</td><td>{}</td><td>{}</td><td><span class="status {}">{}</span></td><td><code>{}</code></td><td>{}</td></tr>"#,
            readback.device_id.as_deref().map(html_escape).unwrap_or_else(|| "Not captured".to_string()),
            readback.repeat_index + 1,
            html_escape(&readback.command),
            if readback.status == "passed" { "status-pass" } else { "status-fail" },
            html_escape(&readback.status),
            html_escape(&values),
            html_escape(&readback.message),
        ));
    }
    if rows.is_empty() {
        rows = "<tr><td colspan=\"6\" style=\"text-align:center;color:#64748b\">Not captured</td></tr>".to_string();
    }
    format!(
        r#"<h2>HC device/readback</h2>
<div class="table-wrap"><table class="env-table"><tbody><tr><th>Devices</th><td>{devices}</td></tr></tbody></table></div>
<div class="table-wrap"><table class="test-table"><thead><tr><th>Device</th><th>Repeat</th><th>Command</th><th>Status</th><th>Readback</th><th>Message</th></tr></thead><tbody>{rows}</tbody></table></div>"#,
        devices = devices,
        rows = rows,
    )
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "Not captured".to_string())
}

/// Builds the conic-gradient string plus centered percentage labels for non-trivial
/// slices (>=8% share, to avoid label clutter on thin slices).
fn render_pie(segments: &[PieSegment], total: f64) -> (String, String) {
    if total <= 0.0 {
        return ("conic-gradient(#e5e7eb 0% 100%)".to_string(), String::new());
    }
    let mut stops = Vec::new();
    let mut labels = String::new();
    let mut cum = 0.0;
    for seg in segments {
        if seg.value <= 0.0 {
            continue;
        }
        let start = cum / total * 100.0;
        cum += seg.value;
        let end = cum / total * 100.0;
        stops.push(format!("{} {:.2}% {:.2}%", seg.color, start, end));

        let share = (end - start) / 100.0;
        if share >= 0.08 {
            let mid_frac = (start / 100.0 + end / 100.0) / 2.0;
            let theta = mid_frac * std::f64::consts::TAU;
            let r = 32.0_f64; // % from center
            let left = 50.0 + r * theta.sin();
            let top = 50.0 - r * theta.cos();
            labels.push_str(&format!(
                r#"<span class="pie-label" style="left:{left:.2}%;top:{top:.2}%" title="{label}: {value}">{pct:.2}%</span>"#,
                left = left,
                top = top,
                label = seg.label,
                value = seg.value as i64,
                pct = share * 100.0
            ));
        }
    }
    (format!("conic-gradient({})", stops.join(", ")), labels)
}

/// Accepts either RFC3339 or the "%Y-%m-%d %H:%M:%S" local-time format that
/// `executor.rs::finish` actually writes into `generated_at`, falling back to
/// `now()` only if both fail to parse.
fn parse_generated_at(s: &str) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&chrono::Local);
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        if let chrono::LocalResult::Single(local_dt) = chrono::Local.from_local_datetime(&naive) {
            return local_dt;
        }
    }
    chrono::Local::now()
}

/// Embeds an image file as a clickable base64 thumbnail so the report stays
/// self-contained (viewable/shareable off the machine it was generated on,
/// unlike a plain `<a href>` to a local filesystem path). Returns "-" if the
/// file can't be read - it never links to a path that likely won't resolve on
/// whatever machine opens this report.
fn embed_image_thumb(path: &str, report_dir: &Path) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return "-".to_string();
    };

    // Embed a genuinely small, re-encoded WebP thumbnail rather than the raw
    // screenshot bytes at CSS-scaled display size - a full-resolution device
    // screenshot (often several hundred KB as PNG) times dozens of failures in one
    // report balloons the HTML file into the tens of MB (confirmed: 30 failures ->
    // ~40MB file, too large to publish/open comfortably). Thumbnail is real
    // pixels, not just a smaller `<img>` box, so the byte size actually shrinks.
    //
    // Lossless WebP (`WebPEncoder::new_lossless`), not lossy: these thumbnails are
    // UI screenshots (flat fills, sharp text edges), the content lossy DCT-based
    // codecs (JPEG) are worst at - lossless WebP's LZ77+Huffman-style compression
    // suits that content and keeps text/edges crisp with no compression artifacts,
    // while still beating PNG on size. Also avoids depending on the "webp-encoder"
    // Cargo feature (that one links libwebp for *lossy* encoding only, and is
    // being phased out upstream) - lossless works with just the "webp" feature.
    let thumb_uri = match image::load_from_memory(&bytes) {
        Ok(img) => {
            let thumb = img.thumbnail(240, 500).to_rgb8();
            let mut buf = Vec::new();
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
            if encoder
                .encode(
                    &thumb,
                    thumb.width(),
                    thumb.height(),
                    image::ColorType::Rgb8,
                )
                .is_ok()
            {
                Some(format!("data:image/webp;base64,{}", STANDARD.encode(&buf)))
            } else {
                None
            }
        }
        Err(_) => None,
    };

    let thumb_uri = thumb_uri.unwrap_or_else(|| {
        // Fallback if decoding failed for some reason (unsupported format, corrupt
        // file) - still show the original rather than nothing.
        let mime = crate::utils::image_convert::guess_image_mime(path);
        format!("data:{};base64,{}", mime, STANDARD.encode(&bytes))
    });

    // The click-through target stays a local file path (not a second, full-res
    // data URI) precisely to avoid re-introducing the file-size blowup - the
    // trade-off is "view full resolution" only works when opened on the machine
    // that generated the report, same as the Log/YAML links elsewhere in this
    // report; the thumbnail itself (the part everyone actually looks at) stays
    // fully self-contained and portable.
    format!(
        r#"<a href="{href}" target="_blank" title="Mở ảnh gốc (file local): {full}"><img class="evidence-thumb" src="{thumb}" alt="evidence" loading="lazy"></a>"#,
        href = html_escape(&relative_href(report_dir, path)),
        full = html_escape(path),
        thumb = thumb_uri
    )
}

/// Renders a path as a link showing just the filename (full path as tooltip,
/// href rewritten relative to the report's own directory) so long absolute
/// paths don't blow out table columns and the link still resolves wherever the
/// report is opened from.
fn short_path_link(path: &str, report_dir: &Path) -> String {
    let basename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    format!(
        r#"<a href="{href}" title="{full}">{short}</a>"#,
        href = html_escape(&relative_href(report_dir, path)),
        full = html_escape(path),
        short = html_escape(basename)
    )
}

fn render_pie_legend(segments: &[PieSegment], total: f64) -> String {
    let mut html = String::new();
    for seg in segments {
        let pct = if total > 0.0 {
            seg.value / total * 100.0
        } else {
            0.0
        };
        html.push_str(&format!(
            r#"<div class="legend-item"><span class="legend-swatch {class}"></span><span>{label}</span><strong>{value} ({pct:.2}%)</strong></div>"#,
            class = seg.class,
            label = seg.label,
            value = seg.value as i64,
            pct = pct
        ));
    }
    html
}
