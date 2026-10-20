use super::types::TestResults;
use crate::runner::state::{CommandStatus, FlowStatus};
use anyhow::Result;
use std::path::Path;

/// Generate HTML report
pub async fn generate(results: &TestResults, output: Option<&Path>) -> Result<()> {
    let html = generate_html(results);

    if let Some(path) = output {
        std::fs::write(path, html)?;
        println!("HTML report saved to: {}", path.display());
    } else {
        println!("{}", html);
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UiElementBox {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub desc: String,
    pub clickable: bool,
}

fn parse_bounds_str(val: &str) -> Option<(i32, i32, i32, i32)> {
    let parts: Vec<&str> = val
        .trim_matches(|c| c == '[' || c == ']')
        .split("][")
        .collect();
    if parts.len() == 2 {
        let p1: Vec<&str> = parts[0].split(',').collect();
        let p2: Vec<&str> = parts[1].split(',').collect();
        if p1.len() == 2 && p2.len() == 2 {
            let left = p1[0].trim().parse::<i32>().ok()?;
            let top = p1[1].trim().parse::<i32>().ok()?;
            let right = p2[0].trim().parse::<i32>().ok()?;
            let bottom = p2[1].trim().parse::<i32>().ok()?;
            return Some((left, top, right, bottom));
        }
    }
    None
}

fn extract_ui_elements(content: &str) -> Vec<UiElementBox> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // 1. Android UIAutomator XML format (<node bounds="[l,t][r,b]" ...)
    if let Ok(elements) = crate::driver::android::uiautomator::parse_hierarchy(trimmed) {
        if !elements.is_empty() {
            let mut list = Vec::new();
            for el in elements {
                if el.bounds.right > el.bounds.left && el.bounds.bottom > el.bounds.top {
                    list.push(UiElementBox {
                        left: el.bounds.left,
                        top: el.bounds.top,
                        right: el.bounds.right,
                        bottom: el.bounds.bottom,
                        text: el.text,
                        id: el.resource_id,
                        class: el.class,
                        desc: el.content_desc,
                        clickable: el.clickable,
                    });
                }
            }
            if !list.is_empty() {
                return list;
            }
        }
    }

    // 2. iOS JSON hierarchy format (idb / WDA)
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(elements) = crate::driver::ios::accessibility::parse_ui_hierarchy(trimmed) {
            let flat = crate::driver::ios::accessibility::flatten_elements(&elements);
            let mut list = Vec::new();
            for el in flat {
                let left = el.frame.x.round() as i32;
                let top = el.frame.y.round() as i32;
                let right = (el.frame.x + el.frame.width).round() as i32;
                let bottom = (el.frame.y + el.frame.height).round() as i32;
                if right > left && bottom > top {
                    let text = el
                        .label
                        .clone()
                        .or_else(|| el.value.clone())
                        .unwrap_or_default();
                    let id = el.identifier.clone().unwrap_or_default();
                    let class = el.element_type.clone().unwrap_or_default();
                    let desc = el.placeholder.clone().unwrap_or_default();
                    let clickable = el.enabled
                        && (class.contains("Button")
                            || class.contains("TextField")
                            || class.contains("Cell")
                            || class.contains("Switch")
                            || class.contains("Link")
                            || class.contains("SegmentedControl"));
                    list.push(UiElementBox {
                        left,
                        top,
                        right,
                        bottom,
                        text,
                        id,
                        class,
                        desc,
                        clickable,
                    });
                }
            }
            if !list.is_empty() {
                return list;
            }
        }
    }

    // 3. Generic XML format for macOS, Windows, and Web hierarchies
    // Matches <element x=".." y=".." width=".." height=".." ... /> and similar variants
    let mut list = Vec::new();
    let mut reader = quick_xml::Reader::from_str(trimmed);
    reader.trim_text(true);
    let mut buf = Vec::new();

    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag_name == "element" || tag_name == "node" || tag_name == "item" {
                    let mut x: Option<i32> = None;
                    let mut y: Option<i32> = None;
                    let mut width: Option<i32> = None;
                    let mut height: Option<i32> = None;
                    let mut left: Option<i32> = None;
                    let mut top: Option<i32> = None;
                    let mut right: Option<i32> = None;
                    let mut bottom: Option<i32> = None;
                    let mut text = String::new();
                    let mut id = String::new();
                    let mut class = String::new();
                    let mut desc = String::new();
                    let mut clickable = false;

                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        let val = String::from_utf8_lossy(&attr.value).to_string();

                        match key.as_str() {
                            "x" => x = val.parse::<f64>().ok().map(|v| v.round() as i32),
                            "y" => y = val.parse::<f64>().ok().map(|v| v.round() as i32),
                            "width" | "w" => {
                                width = val.parse::<f64>().ok().map(|v| v.round() as i32)
                            }
                            "height" | "h" => {
                                height = val.parse::<f64>().ok().map(|v| v.round() as i32)
                            }
                            "left" => left = val.parse::<f64>().ok().map(|v| v.round() as i32),
                            "top" => top = val.parse::<f64>().ok().map(|v| v.round() as i32),
                            "right" => right = val.parse::<f64>().ok().map(|v| v.round() as i32),
                            "bottom" => bottom = val.parse::<f64>().ok().map(|v| v.round() as i32),
                            "bounds" => {
                                if let Some((l, t, r, b)) = parse_bounds_str(&val) {
                                    left = Some(l);
                                    top = Some(t);
                                    right = Some(r);
                                    bottom = Some(b);
                                }
                            }
                            "text" | "name" | "title" | "value" | "label" => {
                                if text.is_empty() && !val.trim().is_empty() {
                                    text = val;
                                }
                            }
                            "id" | "resource-id" | "automation_id" | "identifier" => {
                                if id.is_empty() && !val.trim().is_empty() {
                                    id = val;
                                }
                            }
                            "class" | "type" | "role" | "tag" | "control_type" => {
                                if class.is_empty() && !val.trim().is_empty() {
                                    class = val;
                                }
                            }
                            "description" | "desc" | "content-desc" | "help_text"
                            | "placeholder" => {
                                if desc.is_empty() && !val.trim().is_empty() {
                                    desc = val;
                                }
                            }
                            "clickable" => clickable = val == "true" || val == "1",
                            "enabled" => {
                                if val == "true"
                                    && (class.contains("Button")
                                        || class.contains("Link")
                                        || class.contains("Input"))
                                {
                                    clickable = true;
                                }
                            }
                            _ => {}
                        }
                    }

                    let (final_l, final_t, final_r, final_b) =
                        match (left, top, right, bottom, x, y, width, height) {
                            (Some(l), Some(t), Some(r), Some(b), ..) => (l, t, r, b),
                            (_, _, _, _, Some(px), Some(py), Some(pw), Some(ph)) => {
                                (px, py, px + pw, py + ph)
                            }
                            _ => (0, 0, 0, 0),
                        };

                    if final_r > final_l && final_b > final_t {
                        if !clickable {
                            let lc = class.to_lowercase();
                            if lc.contains("button")
                                || lc.contains("input")
                                || lc.contains("link")
                                || lc.contains("select")
                                || lc.contains("checkbox")
                                || lc.contains("switch")
                            {
                                clickable = true;
                            }
                        }

                        list.push(UiElementBox {
                            left: final_l,
                            top: final_t,
                            right: final_r,
                            bottom: final_b,
                            text,
                            id,
                            class,
                            desc,
                            clickable,
                        });
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    list
}

fn generate_html(results: &TestResults) -> String {
    let summary = &results.summary;
    let pass_rate = if summary.total_commands > 0 {
        (summary.passed as f64 / summary.total_commands as f64 * 100.0) as u32
    } else {
        0
    };

    // Calculate stability matrix per flow
    let mut flow_stats: std::collections::HashMap<String, (usize, usize, usize, u64)> = std::collections::HashMap::new();
    for flow in &results.flows {
        let entry = flow_stats.entry(flow.flow_name.clone()).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        match flow.status {
            FlowStatus::Passed => entry.1 += 1,
            FlowStatus::Failed | FlowStatus::PartiallyPassed { .. } => entry.2 += 1,
            _ => {}
        }
        entry.3 += flow.total_duration_ms.unwrap_or(0);
    }

    let mut stability_table_html = String::new();
    if results.flows.len() > 1 || flow_stats.values().any(|(total, _, _, _)| *total > 1) {
        let mut rows = String::new();
        let mut sorted_stats: Vec<_> = flow_stats.into_iter().collect();
        sorted_stats.sort_by(|a, b| a.0.cmp(&b.0));

        for (flow_name, (total, passed, failed, duration_ms)) in sorted_stats {
            let flow_pass_rate = if total > 0 { (passed as f64 / total as f64 * 100.0) as u32 } else { 0 };
            let (status_badge, status_class) = if failed == 0 {
                ("STABLE (100%)", "passed")
            } else if passed > 0 {
                ("FLAKY", "flaky")
            } else {
                ("FAILING", "failed")
            };

            rows.push_str(&format!(
                r#"
                <tr>
                    <td><span class="flow-pill">{}</span></td>
                    <td><strong>{}</strong> run(s)</td>
                    <td><span style="color: var(--green); font-weight: 600;">{} passed</span> / <span style="color: var(--red); font-weight: 600;">{} failed</span></td>
                    <td><strong>{}%</strong></td>
                    <td>{}</td>
                    <td><span class="status-badge {}">{}</span></td>
                </tr>
                "#,
                html_escape(&flow_name),
                total,
                passed,
                failed,
                flow_pass_rate,
                format_duration(duration_ms),
                status_class,
                status_badge
            ));
        }

        stability_table_html = format!(
            r#"
            <div class="stability-card">
                <div class="card-header">
                    <h3>📊 Flow Execution & Stability Matrix</h3>
                </div>
                <table>
                    <thead>
                        <tr>
                            <th>Flow Name</th>
                            <th>Total Runs</th>
                            <th>Passed / Failed</th>
                            <th>Pass Rate</th>
                            <th>Total Time</th>
                            <th>Stability</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
            "#,
            rows
        );
    }

    // Overview charts: status donut (Passed/Failed/Skipped commands) + top flows by
    // duration. Computed independently of `flow_stats` above (which is consumed by
    // the stability table) rather than cloning it.
    let donut_html = donut_chart_html(
        &[
            ("#10b981", summary.passed as f64),
            ("#ef4444", summary.failed as f64),
            ("#f59e0b", summary.skipped as f64),
        ],
        &format!("{}%", pass_rate),
        "Pass Rate",
    );
    let donut_legend = donut_legend_html(&[
        ("#10b981", "Passed", summary.passed as f64),
        ("#ef4444", "Failed", summary.failed as f64),
        ("#f59e0b", "Skipped", summary.skipped as f64),
    ]);

    let mut duration_by_flow: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for flow in &results.flows {
        *duration_by_flow.entry(flow.flow_name.clone()).or_insert(0) += flow.total_duration_ms.unwrap_or(0);
    }
    let mut duration_sorted: Vec<(String, u64)> = duration_by_flow.into_iter().collect();
    duration_sorted.sort_by(|a, b| b.1.cmp(&a.1));
    duration_sorted.truncate(8);
    let max_duration = duration_sorted.iter().map(|(_, d)| *d).max().unwrap_or(0);
    let duration_bars_html = if duration_sorted.len() > 1 {
        let mut rows = String::new();
        for (name, dur) in &duration_sorted {
            let pct = if max_duration > 0 { (*dur as f64 / max_duration as f64 * 100.0).clamp(2.0, 100.0) } else { 2.0 };
            rows.push_str(&format!(
                r#"<div class="hbar-row">
                    <div class="hbar-label">{label}</div>
                    <div class="hbar-track"><div class="hbar-fill" style="width:{pct:.1}%"></div></div>
                    <div class="hbar-value">{value}</div>
                </div>"#,
                label = html_escape(name),
                pct = pct,
                value = format_duration(*dur)
            ));
        }
        format!(
            r#"<div class="chart-card" style="flex: 1.3;">
                <h3>⏱️ Duration by Flow (Top {})</h3>
                <div class="hbar-list">{rows}</div>
            </div>"#,
            duration_sorted.len(),
            rows = rows
        )
    } else {
        String::new()
    };

    let charts_row_html = format!(
        r#"<div class="charts-row">
            <div class="chart-card">
                <h3>✅ Status Breakdown</h3>
                <div class="donut-wrap">
                    {donut}
                    <div class="legend">{legend}</div>
                </div>
            </div>
            {duration_bars}
        </div>"#,
        donut = donut_html,
        legend = donut_legend,
        duration_bars = duration_bars_html
    );

    let mut flows_html = String::new();
    for (flow_idx, flow) in results.flows.iter().enumerate() {
        let (flow_status_text, flow_status_class) = match flow.status {
            FlowStatus::Passed => ("Passed", "passed"),
            FlowStatus::Failed => ("Failed", "failed"),
            _ => ("Partial", "partial"),
        };

        let mut commands_html = String::new();
        for cmd in &flow.commands {
            let (status_icon, status_class) = match &cmd.status {
                CommandStatus::Passed => ("✓", "passed"),
                CommandStatus::Failed { .. } => ("✗", "failed"),
                CommandStatus::Skipped { .. } => ("○", "skipped"),
                CommandStatus::Running => ("⋯", "running"),
                CommandStatus::Pending => ("○", "pending"),
                CommandStatus::Retrying { .. } => ("↻", "retrying"),
            };

            let retry_html = if cmd.retry_count > 0 {
                format!(r#"<span class="retry-badge">↻ Retried {} time(s)</span>"#, cmd.retry_count)
            } else {
                String::new()
            };

            let duration_html = cmd
                .duration_ms
                .map(|d| format!("<span class=\"duration\">{}ms</span>", d))
                .unwrap_or_default();

            // Rich failure inspector with inline thumbnail, XML hierarchy and recent logs
            let error_html = match &cmd.status {
                CommandStatus::Failed { error } => {
                    let mut ui_elements = Vec::new();
                    let mut hierarchy_content = String::new();

                    if let Some(path) = &cmd.ui_hierarchy_path {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            ui_elements = extract_ui_elements(&content);
                            hierarchy_content = content;
                        }
                    }

                    use base64::{engine::general_purpose::STANDARD, Engine};
                    let elements_json = serde_json::to_string(&ui_elements).unwrap_or_else(|_| "[]".to_string());
                    let elements_b64 = STANDARD.encode(elements_json.as_bytes());
                    let element_count = ui_elements.len();

                    let screenshot_box = if let Some(path) = &cmd.screenshot_path {
                        // Embed a downscaled, re-encoded WebP copy rather than the raw
                        // full-resolution screenshot - a report with many failures
                        // otherwise balloons into tens of MB of base64 (same issue
                        // `report::summary_html::embed_image_thumb` fixed for the
                        // dashboard view). The bounding-box overlay in the modal is
                        // driven by percentages (left/width), so it stays pixel-exact
                        // regardless of this resize as long as the ORIGINAL width/height
                        // is passed through separately (see `showScreenshot` below) -
                        // it never relies on the displayed image's own resolution.
                        let (img_src, orig_w, orig_h) = match std::fs::read(path) {
                            Ok(bytes) => match image::load_from_memory(&bytes) {
                                Ok(decoded) => {
                                    let (w, h) = (decoded.width(), decoded.height());
                                    let thumb = decoded.thumbnail(720, 1440).to_rgb8();
                                    let mut buf = Vec::new();
                                    let encoder =
                                        image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
                                    let src = if encoder
                                        .encode(
                                            &thumb,
                                            thumb.width(),
                                            thumb.height(),
                                            image::ColorType::Rgb8,
                                        )
                                        .is_ok()
                                    {
                                        format!(
                                            "data:image/webp;base64,{}",
                                            STANDARD.encode(&buf)
                                        )
                                    } else {
                                        let mime =
                                            crate::utils::image_convert::guess_image_mime(path);
                                        format!("data:{};base64,{}", mime, STANDARD.encode(&bytes))
                                    };
                                    (src, w, h)
                                }
                                Err(_) => {
                                    let mime = crate::utils::image_convert::guess_image_mime(path);
                                    (
                                        format!("data:{};base64,{}", mime, STANDARD.encode(&bytes)),
                                        0,
                                        0,
                                    )
                                }
                            },
                            Err(_) => (
                                std::path::Path::new(path)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(path)
                                    .to_string(),
                                0,
                                0,
                            ),
                        };

                        let count_badge = if element_count > 0 {
                            format!(r#"<div class="elem-badge">🎯 {} UI boxes</div>"#, element_count)
                        } else {
                            String::new()
                        };

                        format!(
                            r#"<div class="evidence-thumb-box" data-elements="{elements_b64}" onclick="showScreenshot(this.querySelector('img').src, this.getAttribute('data-elements'), {orig_w}, {orig_h})">
                                <img src="{img_src}" alt="Failure Snapshot" />
                                <div class="zoom-label">🔍 Inspect UI Bounding Boxes</div>
                                {count_badge}
                            </div>"#,
                            img_src = img_src,
                            elements_b64 = elements_b64,
                            count_badge = count_badge,
                            orig_w = orig_w,
                            orig_h = orig_h,
                        )
                    } else {
                        String::new()
                    };

                    let hierarchy_box = if let Some(path) = &cmd.ui_hierarchy_path {
                        let snippet = if !hierarchy_content.is_empty() {
                            let lines: Vec<&str> = hierarchy_content.lines().collect();
                            if lines.len() > 60 {
                                format!("{}\n<!-- ... ({} more lines) ... -->", lines[..60].join("\n"), lines.len() - 60)
                            } else {
                                hierarchy_content.clone()
                            }
                        } else {
                            format!("Path: {}", path)
                        };
                        format!(
                            r#"<details class="evidence-details">
                                <summary>📄 View UI Hierarchy XML ({} elements detected)</summary>
                                <pre><code>{snippet}</code></pre>
                            </details>"#,
                            element_count,
                            snippet = html_escape(&snippet)
                        )
                    } else {
                        String::new()
                    };

                    let logs_box = if let Some(path) = &cmd.log_path {
                        let snippet = match std::fs::read_to_string(path) {
                            Ok(content) => {
                                let lines: Vec<&str> = content.lines().collect();
                                if lines.len() > 50 {
                                    lines[lines.len() - 50..].join("\n")
                                } else {
                                    content
                                }
                            }
                            Err(_) => format!("Path: {}", path),
                        };
                        format!(
                            r#"<details class="evidence-details">
                                <summary>📋 View Recent Device Logs ({path})</summary>
                                <pre><code>{snippet}</code></pre>
                            </details>"#,
                            path = path,
                            snippet = html_escape(&snippet)
                        )
                    } else {
                        String::new()
                    };

                    format!(
                        r##"
                        <div class="failure-inspector">
                            <div class="error-header">
                                <span class="error-badge">FAILURE DETAILS</span>
                                <span class="cmd-idx">Step #{}</span>
                            </div>
                            <div class="error-message">{}</div>
                            {}
                            <div class="evidence-grid">
                                {}
                                <div class="evidence-texts">
                                    {}
                                    {}
                                </div>
                            </div>
                        </div>
                        "##,
                        cmd.index,
                        html_escape(error),
                        camera_hint_html(error),
                        screenshot_box,
                        hierarchy_box,
                        logs_box
                    )
                }
                _ => String::new(),
            };

            commands_html.push_str(&format!(
                r##"
                <div class="command {status_class}">
                    <div class="command-icon">{status_icon}</div>
                    <div class="command-content">
                        <div class="command-title-row">
                            <span class="command-name">[{}] {}</span>
                            <div class="command-meta">
                                {retry_html}
                                {duration_html}
                            </div>
                        </div>
                        {error_html}
                    </div>
                </div>
            "##,
                cmd.index,
                html_escape(&cmd.command_display),
                status_class = status_class,
                status_icon = status_icon,
                retry_html = retry_html,
                duration_html = duration_html,
                error_html = error_html
            ));
        }

        let duration_html = flow
            .total_duration_ms
            .map(|d| format!("<span class=\"duration\">{}ms</span>", d))
            .unwrap_or_default();

        let video_html = if let Some(path) = &flow.video_path {
            format!(
                r#"
                <div class="video-details">
                    <details>
                        <summary>🎥 View Execution Video</summary>
                        <video controls preload="metadata">
                            <source src="{}" type="video/mp4">
                            Your browser does not support the video tag.
                        </video>
                    </details>
                </div>
            "#,
                path
            )
        } else {
            String::new()
        };

        flows_html.push_str(&format!(
            r#"
            <div class="flow {flow_status_class}">
                <div class="flow-header">
                    <h3>#{run_num}: {} <span class="flow-status-badge">{flow_status_text}</span></h3>
                    {duration_html}
                </div>
                <div class="commands">
                    {commands_html}
                </div>
                {video_html}
            </div>
        "#,
            html_escape(&flow.flow_name),
            run_num = flow_idx + 1,
            video_html = video_html
        ));
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Test Report - {}</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-primary: #0a0f1d;
            --bg-secondary: #141b2d;
            --bg-tertiary: #1f2937;
            --border: #374151;
            --text-primary: #f9fafb;
            --text-secondary: #9ca3af;
            --green: #10b981;
            --red: #ef4444;
            --yellow: #f59e0b;
            --blue: #3b82f6;
            --purple: #8b5cf6;
            --glass: rgba(255, 255, 255, 0.03);
        }}
        
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        
        body {{
            font-family: 'Inter', system-ui, -apple-system, sans-serif;
            background: var(--bg-primary);
            color: var(--text-primary);
            line-height: 1.5;
            padding: 3rem 1rem;
        }}
        
        .container {{
            max-width: 1100px;
            margin: 0 auto;
        }}
        
        header {{
            margin-bottom: 3rem;
            display: flex;
            justify-content: space-between;
            align-items: flex-end;
        }}
        
        h1 {{
            font-size: 2.25rem;
            font-weight: 800;
            letter-spacing: -0.025em;
            background: linear-gradient(135deg, #fff 0%, #94a3b8 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }}
        
        .summary {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 1.5rem;
            margin-bottom: 3rem;
        }}
        
        .stat {{
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            padding: 1.5rem;
            border-radius: 1rem;
            position: relative;
            overflow: hidden;
            transition: transform 0.2s;
        }}
        
        .stat:hover {{
            transform: translateY(-2px);
        }}
        
        .stat-value {{
            font-size: 2.5rem;
            font-weight: 800;
            margin-bottom: 0.25rem;
        }}
        
        .stat-label {{
            color: var(--text-secondary);
            font-size: 0.875rem;
            font-weight: 500;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }}
        
        .stat.passed .stat-value {{ color: var(--green); }}
        .stat.failed .stat-value {{ color: var(--red); }}
        .stat.skipped .stat-value {{ color: var(--yellow); }}
        
        .progress-container {{
            margin-bottom: 4rem;
        }}
        
        .progress-bar {{
            background: var(--bg-secondary);
            height: 12px;
            border-radius: 6px;
            overflow: hidden;
            display: flex;
            border: 1px solid var(--border);
        }}
        
        .progress-fill {{
            height: 100%;
            background: linear-gradient(90deg, var(--green), #34d399);
            transition: width 0.8s cubic-bezier(0.16, 1, 0.3, 1);
        }}

        .charts-row {{
            display: flex;
            gap: 1.5rem;
            margin: 2rem 0;
            flex-wrap: wrap;
        }}
        .chart-card {{
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 1rem;
            padding: 1.5rem;
            flex: 1;
            min-width: 280px;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        }}
        .chart-card h3 {{
            font-size: 0.8125rem;
            font-weight: 700;
            margin-bottom: 1.25rem;
            color: var(--text-secondary);
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }}
        .donut-wrap {{
            display: flex;
            align-items: center;
            gap: 1.75rem;
            flex-wrap: wrap;
        }}
        .donut {{
            width: 140px;
            height: 140px;
            border-radius: 50%;
            flex-shrink: 0;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .donut-hole {{
            width: 68%;
            height: 68%;
            border-radius: 50%;
            background: var(--bg-secondary);
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
        }}
        .donut-value {{
            font-size: 1.5rem;
            font-weight: 800;
        }}
        .donut-label {{
            font-size: 0.6875rem;
            color: var(--text-secondary);
            text-transform: uppercase;
            letter-spacing: 0.05em;
            margin-top: 0.15rem;
        }}
        .legend {{
            display: flex;
            flex-direction: column;
            gap: 0.6rem;
            min-width: 120px;
        }}
        .legend-item {{
            display: flex;
            align-items: center;
            gap: 0.5rem;
            font-size: 0.8125rem;
            color: var(--text-primary);
        }}
        .legend-dot {{
            width: 10px;
            height: 10px;
            border-radius: 50%;
            flex-shrink: 0;
        }}
        .legend-count {{
            margin-left: auto;
            font-weight: 700;
            font-family: 'JetBrains Mono', monospace;
        }}
        .hbar-list {{
            display: flex;
            flex-direction: column;
            gap: 0.75rem;
        }}
        .hbar-row {{
            display: grid;
            grid-template-columns: minmax(0, 1fr) 2fr auto;
            align-items: center;
            gap: 0.75rem;
        }}
        .hbar-label {{
            font-size: 0.8125rem;
            color: var(--text-secondary);
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }}
        .hbar-track {{
            height: 10px;
            background: #1f2937;
            border-radius: 9999px;
            overflow: hidden;
        }}
        .hbar-fill {{
            height: 100%;
            border-radius: 9999px;
            background: linear-gradient(90deg, var(--purple), var(--blue));
        }}
        .hbar-value {{
            font-size: 0.75rem;
            font-weight: 700;
            font-family: 'JetBrains Mono', monospace;
            color: var(--text-secondary);
            white-space: nowrap;
        }}
        .trend-chart {{
            display: flex;
            align-items: flex-end;
            gap: 4px;
            height: 90px;
        }}
        .trend-bar-wrap {{
            flex: 1;
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: flex-end;
            height: 100%;
            min-width: 5px;
        }}
        .trend-bar {{
            width: 100%;
            border-radius: 3px 3px 0 0;
            min-height: 3px;
            transition: opacity 0.15s;
        }}
        .trend-bar-wrap:hover .trend-bar {{
            opacity: 0.7;
        }}

        .flow {{
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 1.25rem;
            margin-bottom: 2rem;
            overflow: hidden;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        }}
        
        .flow-header {{
            padding: 1.5rem;
            background: var(--glass);
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid var(--border);
        }}
        
        .flow-header h3 {{
            font-size: 1.25rem;
            font-weight: 700;
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }}
        
        .flow-status-badge {{
            padding: 0.25rem 0.75rem;
            border-radius: 9999px;
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
        }}
        
        .flow.passed .flow-status-badge {{ background: rgba(16, 185, 129, 0.1); color: var(--green); }}
        .flow.failed .flow-status-badge {{ background: rgba(239, 68, 68, 0.1); color: var(--red); }}
        
        .commands {{
            padding: 1rem 1.5rem;
        }}
        
        .command {{
            padding: 1rem;
            border-radius: 0.75rem;
            display: flex;
            align-items: flex-start;
            gap: 1rem;
            margin-bottom: 0.5rem;
            transition: background 0.2s;
            cursor: pointer;
        }}
        
        .command:hover {{
            background: var(--bg-tertiary);
        }}
        
        .command-icon {{
            width: 2rem;
            height: 2rem;
            display: flex;
            align-items: center;
            justify-content: center;
            border-radius: 0.5rem;
            font-size: 1.25rem;
            flex-shrink: 0;
        }}
        
        .command.passed .command-icon {{ background: rgba(16, 185, 129, 0.1); color: var(--green); }}
        .command.failed .command-icon {{ background: rgba(239, 68, 68, 0.1); color: var(--red); }}
        .command.skipped .command-icon {{ background: rgba(245, 158, 11, 0.1); color: var(--yellow); }}
        
        .command-content {{
            flex: 1;
        }}
        
        .command-name {{
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.9375rem;
            font-weight: 500;
            color: var(--text-primary);
        }}
        
        .command-title-row {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            flex-wrap: wrap;
            gap: 0.5rem;
        }}

        .retry-badge {{
            background: rgba(245, 158, 11, 0.15);
            color: var(--yellow);
            padding: 0.15rem 0.5rem;
            border-radius: 9999px;
            font-size: 0.75rem;
            font-weight: 700;
        }}

        .stability-card {{
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 1rem;
            margin-bottom: 2.5rem;
            overflow: hidden;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        }}

        .card-header {{
            padding: 1rem 1.5rem;
            background: var(--glass);
            border-bottom: 1px solid var(--border);
        }}

        .card-header h3 {{
            font-size: 1.125rem;
            font-weight: 700;
        }}

        table {{
            width: 100%;
            border-collapse: collapse;
            text-align: left;
        }}

        th {{
            padding: 0.875rem 1.25rem;
            font-size: 0.75rem;
            font-weight: 700;
            color: var(--text-secondary);
            text-transform: uppercase;
            letter-spacing: 0.05em;
            border-bottom: 1px solid var(--border);
            background: var(--glass);
        }}

        td {{
            padding: 0.875rem 1.25rem;
            border-bottom: 1px solid rgba(51, 65, 85, 0.4);
            font-size: 0.875rem;
        }}

        .status-badge {{
            display: inline-block;
            padding: 0.2rem 0.5rem;
            border-radius: 9999px;
            font-size: 0.75rem;
            font-weight: 700;
            text-transform: uppercase;
        }}
        .status-badge.passed {{ background: rgba(16, 185, 129, 0.15); color: var(--green); }}
        .status-badge.failed {{ background: rgba(239, 68, 68, 0.15); color: var(--red); }}
        .status-badge.flaky {{ background: rgba(245, 158, 11, 0.15); color: var(--yellow); }}

        .flow-pill {{
            display: inline-block;
            background: rgba(139, 92, 246, 0.15);
            color: var(--purple);
            padding: 0.2rem 0.5rem;
            border-radius: 0.375rem;
            font-weight: 600;
            font-size: 0.8125rem;
        }}

        /* Failure Inspector Styles */
        .failure-inspector {{
            background: rgba(239, 68, 68, 0.07);
            border: 1px solid rgba(239, 68, 68, 0.25);
            border-radius: 0.75rem;
            padding: 1rem;
            margin-top: 0.75rem;
        }}

        .error-header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 0.5rem;
        }}

        .error-badge {{
            font-size: 0.75rem;
            font-weight: 800;
            color: #ef4444;
            letter-spacing: 0.05em;
        }}

        .cmd-idx {{
            font-size: 0.75rem;
            color: var(--text-secondary);
            font-family: 'JetBrains Mono', monospace;
        }}

        .evidence-grid {{
            display: grid;
            grid-template-columns: auto 1fr;
            gap: 1rem;
            margin-top: 0.75rem;
            align-items: flex-start;
        }}

        @media (max-width: 768px) {{
            .evidence-grid {{
                grid-template-columns: 1fr;
            }}
        }}

        .evidence-thumb-box {{
            position: relative;
            cursor: pointer;
            border-radius: 0.5rem;
            overflow: hidden;
            border: 1px solid var(--border);
            max-width: 200px;
            background: #000;
        }}

        .evidence-thumb-box img {{
            width: 100%;
            height: auto;
            display: block;
            transition: transform 0.2s;
        }}

        .evidence-thumb-box:hover img {{
            transform: scale(1.03);
        }}

        .zoom-label {{
            position: absolute;
            bottom: 0;
            left: 0;
            right: 0;
            background: rgba(0, 0, 0, 0.75);
            color: #fff;
            font-size: 0.6875rem;
            padding: 0.25rem;
            text-align: center;
            font-weight: 600;
        }}

        .elem-badge {{
            position: absolute;
            top: 0.35rem;
            right: 0.35rem;
            background: rgba(16, 185, 129, 0.95);
            color: #000;
            font-size: 0.625rem;
            font-weight: 800;
            padding: 0.15rem 0.4rem;
            border-radius: 9999px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.5);
        }}

        .evidence-texts {{
            flex: 1;
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }}

        .evidence-details {{
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 0.5rem;
            padding: 0.5rem 0.75rem;
            font-size: 0.8125rem;
        }}

        .evidence-details summary {{
            cursor: pointer;
            color: var(--blue);
            font-weight: 600;
            user-select: none;
            outline: none;
        }}

        .evidence-details pre {{
            margin-top: 0.5rem;
            padding: 0.5rem;
            background: #000;
            border-radius: 0.375rem;
            overflow-x: auto;
            max-height: 250px;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.75rem;
            color: #94a3b8;
            white-space: pre-wrap;
            word-break: break-all;
        }}

        .meta {{
            margin-top: 4rem;
            padding-top: 2rem;
            border-top: 1px solid var(--border);
            color: var(--text-secondary);
            font-size: 0.875rem;
            text-align: center;
            display: flex;
            justify-content: center;
            gap: 2rem;
        }}
        
        /* Interactive Visual Inspector Modal */
        #modal {{
            display: none;
            position: fixed;
            z-index: 1000;
            top: 0;
            left: 0;
            width: 100vw;
            height: 100vh;
            background: rgba(15, 23, 42, 0.95);
            backdrop-filter: blur(8px);
            padding: 1.5rem;
            align-items: center;
            justify-content: center;
            box-sizing: border-box;
        }}

        #modal.active {{
            display: flex;
        }}

        .modal-card {{
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 1rem;
            width: 95vw;
            max-width: 1300px;
            height: 90vh;
            display: flex;
            flex-direction: column;
            box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.7);
            overflow: hidden;
        }}

        .modal-header {{
            padding: 0.875rem 1.5rem;
            background: var(--glass);
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}

        .modal-title {{
            font-size: 1.125rem;
            font-weight: 700;
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }}

        .modal-controls {{
            display: flex;
            align-items: center;
            gap: 1rem;
        }}

        .toggle-btn {{
            display: flex;
            align-items: center;
            gap: 0.5rem;
            background: #1e293b;
            border: 1px solid var(--border);
            color: #f8fafc;
            padding: 0.35rem 0.75rem;
            border-radius: 0.375rem;
            font-size: 0.8125rem;
            cursor: pointer;
            user-select: none;
        }}

        .toggle-btn input {{
            cursor: pointer;
        }}

        .modal-close-btn {{
            background: #ef4444;
            color: #fff;
            border: none;
            padding: 0.4rem 0.85rem;
            border-radius: 0.375rem;
            font-size: 0.8125rem;
            font-weight: 700;
            cursor: pointer;
            transition: opacity 0.2s;
        }}
        .modal-close-btn:hover {{
            opacity: 0.9;
        }}

        .modal-body {{
            display: flex;
            flex: 1;
            overflow: hidden;
            padding: 1rem;
            gap: 1rem;
        }}

        .modal-viewport {{
            flex: 1;
            display: flex;
            align-items: center;
            justify-content: center;
            background: #090d16;
            border-radius: 0.5rem;
            border: 1px solid var(--border);
            overflow: auto;
            position: relative;
            padding: 0.5rem;
        }}

        .screen-container {{
            position: relative;
            display: inline-block;
            max-height: 100%;
        }}

        #modal-img {{
            max-height: calc(90vh - 130px);
            max-width: 100%;
            height: auto;
            display: block;
            border-radius: 0.375rem;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.5);
            background: #000;
        }}

        .bbox-overlay {{
            position: absolute;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            pointer-events: auto;
        }}

        .bbox-box {{
            position: absolute;
            box-sizing: border-box;
            border: 1.5px solid rgba(139, 92, 246, 0.7);
            background: rgba(139, 92, 246, 0.08);
            border-radius: 2px;
            cursor: crosshair;
            transition: all 0.15s;
        }}

        .bbox-box.has-text {{
            border-color: rgba(16, 185, 129, 0.85);
            background: rgba(16, 185, 129, 0.12);
        }}

        .bbox-box.clickable {{
            border-color: rgba(59, 130, 246, 0.85);
            background: rgba(59, 130, 246, 0.12);
        }}

        .bbox-box:hover, .bbox-box.selected {{
            border: 2.5px solid #facc15 !important;
            background: rgba(250, 204, 21, 0.35) !important;
            z-index: 50 !important;
            box-shadow: 0 0 12px rgba(250, 204, 21, 0.8);
        }}

        .modal-sidebar {{
            width: 360px;
            min-width: 300px;
            background: #141b2d;
            border: 1px solid var(--border);
            border-radius: 0.5rem;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }}

        .sidebar-header {{
            padding: 0.75rem 1rem;
            border-bottom: 1px solid var(--border);
            background: var(--glass);
        }}

        .sidebar-search {{
            width: 100%;
            background: #0f172a;
            border: 1px solid var(--border);
            border-radius: 0.375rem;
            color: #f8fafc;
            padding: 0.4rem 0.6rem;
            font-size: 0.8125rem;
            margin-top: 0.5rem;
            outline: none;
            box-sizing: border-box;
        }}

        .elem-list {{
            flex: 1;
            overflow-y: auto;
            padding: 0.5rem;
        }}

        .elem-card {{
            background: #1e293b;
            border: 1px solid #334155;
            border-radius: 0.375rem;
            padding: 0.5rem;
            margin-bottom: 0.4rem;
            font-size: 0.75rem;
            cursor: pointer;
            transition: all 0.15s;
        }}

        .elem-card:hover, .elem-card.selected {{
            border-color: #facc15;
            background: #283548;
            transform: translateX(2px);
        }}

        .elem-card-title {{
            font-weight: 700;
            color: #f8fafc;
            margin-bottom: 0.2rem;
            word-break: break-all;
        }}

        .elem-card-meta {{
            color: #94a3b8;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.6875rem;
            line-height: 1.3;
        }}

        #bbox-tooltip {{
            position: fixed;
            z-index: 2000;
            background: #0f172a;
            border: 1px solid #3b82f6;
            border-radius: 0.5rem;
            padding: 0.6rem 0.8rem;
            box-shadow: 0 10px 25px -5px rgba(0,0,0,0.8);
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.75rem;
            color: #f8fafc;
            pointer-events: none;
            display: none;
            max-width: 350px;
            word-break: break-all;
            line-height: 1.4;
        }}
        
        .video-details {{
            margin: 0rem 1.5rem 1rem 1.5rem;
            padding: 1rem;
            background: rgba(0, 0, 0, 0.2);
            border-radius: 0.75rem;
            border: 1px solid var(--border);
        }}
        
        .video-details summary {{
            cursor: pointer;
            font-weight: 600;
            color: var(--blue);
            outline: none;
            user-select: none;
            list-style: none; /* Hide default triangle */
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }}
        
        .video-details summary::-webkit-details-marker {{
            display: none;
        }}
        
        .video-details video {{
            margin-top: 1rem;
            border-radius: 0.5rem;
            width: 100%;
            max-width: 800px;
            display: block;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.3);
            background: #000;
        }}

        .camera-hint {{
            background: rgba(59, 130, 246, 0.1);
            border: 1px solid rgba(59, 130, 246, 0.25);
            border-radius: 0.5rem;
            padding: 0.75rem;
            margin-top: 0.75rem;
            color: #bfdbfe;
            font-size: 0.8125rem;
        }}

        .camera-hint pre {{
            white-space: pre-wrap;
            margin-top: 0.5rem;
            font-family: 'JetBrains Mono', monospace;
        }}
    </style>
</head>
<body>
    <div class="container">
        <header>
            <div>
                <div style="font-size: 0.875rem; font-weight: 600; color: var(--purple); text-transform: uppercase; letter-spacing: 0.1em; margin-bottom: 0.5rem;">Automated Testing</div>
                <h1>Test Execution Report</h1>
            </div>
            <div style="text-align: right;">
                <div style="font-size: 0.875rem; color: var(--text-secondary);">Session Duration</div>
                <div style="font-size: 1.25rem; font-weight: 700;">{}</div>
            </div>
        </header>
        
        <div class="summary">
            <div class="stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Total Flows</div>
            </div>
            <div class="stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Total Commands</div>
            </div>
            <div class="stat passed">
                <div class="stat-value">{}</div>
                <div class="stat-label">Passed</div>
            </div>
            <div class="stat failed">
                <div class="stat-value">{}</div>
                <div class="stat-label">Failed</div>
            </div>
        </div>
        
        <div class="progress-container">
            <div style="display: flex; justify-content: space-between; margin-bottom: 0.75rem;">
                <span style="font-weight: 600; font-size: 0.875rem;">Success Rate</span>
                <span style="font-weight: 700; color: var(--green);">{pass_rate}%</span>
            </div>
            <div class="progress-bar">
                <div class="progress-fill" style="width: {pass_rate}%"></div>
            </div>
        </div>

        {charts_row_html}

        {stability_table_html}
        
        {flows_html}
        
        <div class="meta">
            <span>Session: {}</span>
            <span>Generated: {}</span>
        </div>
    </div>

    <div id="modal" onclick="closeModal(event)">
        <div class="modal-card" onclick="event.stopPropagation()">
            <div class="modal-header">
                <div class="modal-title">
                    <span>🔍 UI Hierarchy Visual Inspector</span>
                    <span id="modal-elem-count-badge" style="font-size: 0.75rem; background: rgba(139, 92, 246, 0.2); color: var(--purple); padding: 0.2rem 0.5rem; border-radius: 9999px; font-weight: 700;">0 Elements</span>
                </div>
                <div class="modal-controls">
                    <label class="toggle-btn">
                        <input type="checkbox" id="bboxToggle" checked onchange="toggleBboxes(this.checked)">
                        <span>👁️ Bounding Boxes</span>
                    </label>
                    <button class="modal-close-btn" onclick="document.getElementById('modal').classList.remove('active')">✕ Close</button>
                </div>
            </div>
            <div class="modal-body">
                <div class="modal-viewport">
                    <div class="screen-container" id="screenContainer">
                        <img id="modal-img" src="" alt="Screenshot" onload="renderVisualInspector()">
                        <div id="modal-bbox-overlay" class="bbox-overlay"></div>
                    </div>
                </div>
                <div class="modal-sidebar">
                    <div class="sidebar-header">
                        <div style="font-size: 0.8125rem; font-weight: 700; color: #f8fafc;">Detected UI Elements</div>
                        <input type="text" id="elemSearch" class="sidebar-search" placeholder="Search by text, id, class..." onkeyup="filterElementsList()">
                    </div>
                    <div id="elemList" class="elem-list"></div>
                </div>
            </div>
        </div>
    </div>
    <div id="bbox-tooltip"></div>

    <script>
        let currentElements = [];
        let currentOrigWidth = 0;
        let currentOrigHeight = 0;
        let showBboxes = true;
        let selectedElementIndex = -1;

        function showScreenshot(src, elementsB64, origWidth, origHeight) {{
            const modal = document.getElementById('modal');
            const img = document.getElementById('modal-img');
            currentOrigWidth = origWidth || 0;
            currentOrigHeight = origHeight || 0;
            
            try {{
                currentElements = elementsB64 ? JSON.parse(atob(elementsB64)) : [];
            }} catch(e) {{
                console.error('Failed to parse elements:', e);
                currentElements = [];
            }}

            selectedElementIndex = -1;
            document.getElementById('elemSearch').value = '';
            document.getElementById('modal-elem-count-badge').textContent = currentElements.length + ' Elements';

            img.src = src;
            modal.classList.add('active');
            if (img.complete) {{
                renderVisualInspector();
            }}
            if (window.event) {{
                window.event.stopPropagation();
            }}
        }}

        function closeModal(event) {{
            const modal = document.getElementById('modal');
            modal.classList.remove('active');
            hideBboxTooltip();
        }}

        document.addEventListener('keydown', (e) => {{
            if (e.key === 'Escape') {{
                closeModal();
            }}
        }});

        function toggleBboxes(checked) {{
            showBboxes = checked;
            const overlay = document.getElementById('modal-bbox-overlay');
            overlay.style.display = showBboxes ? 'block' : 'none';
        }}

        function renderVisualInspector() {{
            const img = document.getElementById('modal-img');
            const overlay = document.getElementById('modal-bbox-overlay');
            const elemList = document.getElementById('elemList');
            
            overlay.innerHTML = '';
            elemList.innerHTML = '';

            // Prefer the ORIGINAL screenshot dimensions over the displayed
            // image's own natural size - the embedded image may be a
            // downscaled thumbnail (see html.rs screenshot_box), but element
            // bounds from the hierarchy dump are always in original-pixel
            // space, and this is a ratio (left/width), so using the original
            // width/height as the denominator keeps every box aligned
            // exactly regardless of the thumbnail's actual resolution.
            const naturalWidth = currentOrigWidth || img.naturalWidth || 1080;
            const naturalHeight = currentOrigHeight || img.naturalHeight || 1920;

            currentElements.forEach((el, idx) => {{
                const w = el.right - el.left;
                const h = el.bottom - el.top;
                if (w <= 0 || h <= 0) return;

                const leftPct = (el.left / naturalWidth) * 100;
                const topPct = (el.top / naturalHeight) * 100;
                const widthPct = (w / naturalWidth) * 100;
                const heightPct = (h / naturalHeight) * 100;

                // Create Overlay Box
                const box = document.createElement('div');
                box.className = 'bbox-box' + (el.clickable ? ' clickable' : '') + (el.text ? ' has-text' : '');
                box.id = 'bbox-box-' + idx;
                box.style.left = leftPct + '%';
                box.style.top = topPct + '%';
                box.style.width = widthPct + '%';
                box.style.height = heightPct + '%';

                box.onmouseenter = (e) => {{
                    highlightElement(idx, true);
                    showBboxTooltip(e, el);
                }};
                box.onmousemove = (e) => {{
                    moveBboxTooltip(e);
                }};
                box.onmouseleave = () => {{
                    highlightElement(-1, false);
                    hideBboxTooltip();
                }};
                box.onclick = (e) => {{
                    selectElement(idx);
                    e.stopPropagation();
                }};

                overlay.appendChild(box);

                // Create Sidebar Card
                const card = document.createElement('div');
                card.className = 'elem-card';
                card.id = 'elem-card-' + idx;
                card.setAttribute('data-search', (el.text + ' ' + el.id + ' ' + el.class + ' ' + el.desc).toLowerCase());

                const label = el.text ? ('🔤 ' + el.text) : (el.desc ? ('💬 ' + el.desc) : (el.id ? ('🆔 ' + el.id.split('/').pop()) : ('📦 ' + el.class.split('.').pop())));
                const idText = el.id ? el.id : (el.class ? el.class : '');
                const boundsText = `[${{el.left}}, ${{el.top}}][${{el.right}}, ${{el.bottom}}] (${{w}}x${{h}})`;

                card.innerHTML = `
                    <div class="elem-card-title">${{escapeHtml(label)}}</div>
                    <div class="elem-card-meta">
                        ${{el.id ? `<div><strong>ID:</strong> ${{escapeHtml(el.id)}}</div>` : ''}}
                        <div><strong>Class:</strong> ${{escapeHtml(el.class)}}</div>
                        <div><strong>Bounds:</strong> ${{boundsText}}</div>
                        ${{el.clickable ? '<span style="color: var(--blue); font-weight: 700;">• Clickable</span>' : ''}}
                    </div>
                `;

                card.onmouseenter = () => highlightElement(idx, true);
                card.onmouseleave = () => highlightElement(-1, false);
                card.onclick = (e) => {{
                    selectElement(idx);
                    e.stopPropagation();
                }};

                elemList.appendChild(card);
            }});
        }}

        function highlightElement(idx, active) {{
            document.querySelectorAll('.bbox-box').forEach(b => b.classList.remove('selected'));
            document.querySelectorAll('.elem-card').forEach(c => c.classList.remove('selected'));

            if (active && idx >= 0) {{
                const box = document.getElementById('bbox-box-' + idx);
                if (box) box.classList.add('selected');

                const card = document.getElementById('elem-card-' + idx);
                if (card) {{
                    card.classList.add('selected');
                    card.scrollIntoView({{ behavior: 'smooth', block: 'nearest' }});
                }}
            }}
        }}

        function selectElement(idx) {{
            selectedElementIndex = idx;
            highlightElement(idx, true);
        }}

        function showBboxTooltip(e, el) {{
            const tooltip = document.getElementById('bbox-tooltip');
            const w = el.right - el.left;
            const h = el.bottom - el.top;

            tooltip.innerHTML = `
                <div style="font-weight: 700; color: #facc15; margin-bottom: 0.25rem;">
                    ${{el.text ? `"${{escapeHtml(el.text)}}"` : (el.desc ? `[${{escapeHtml(el.desc)}}]` : escapeHtml(el.class.split('.').pop()))}}
                </div>
                ${{el.id ? `<div><strong style="color: #93c5fd;">ID:</strong> ${{escapeHtml(el.id)}}</div>` : ''}}
                <div><strong style="color: #93c5fd;">Class:</strong> ${{escapeHtml(el.class)}}</div>
                <div><strong style="color: #93c5fd;">Bounds:</strong> [${{el.left}}, ${{el.top}}][${{el.right}}, ${{el.bottom}}] (${{w}}x${{h}}px)</div>
                ${{el.clickable ? '<div style="color: #34d399; font-weight: 700; margin-top: 0.2rem;">✓ Clickable Element</div>' : ''}}
            `;
            tooltip.style.display = 'block';
            moveBboxTooltip(e);
        }}

        function moveBboxTooltip(e) {{
            const tooltip = document.getElementById('bbox-tooltip');
            let x = e.clientX + 15;
            let y = e.clientY + 15;
            if (x + 320 > window.innerWidth) x = e.clientX - 330;
            if (y + 150 > window.innerHeight) y = e.clientY - 160;
            tooltip.style.left = x + 'px';
            tooltip.style.top = y + 'px';
        }}

        function hideBboxTooltip() {{
            const tooltip = document.getElementById('bbox-tooltip');
            tooltip.style.display = 'none';
        }}

        function filterElementsList() {{
            const search = document.getElementById('elemSearch').value.toLowerCase();
            const cards = document.querySelectorAll('.elem-card');
            cards.forEach(card => {{
                const text = card.getAttribute('data-search');
                card.style.display = text.includes(search) ? '' : 'none';
            }});
        }}

        function escapeHtml(s) {{
            if (!s) return '';
            return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
        }}
    </script>
</body>
</html>"#,
        summary.session_id,
        format_duration(summary.total_duration_ms.unwrap_or(0)),
        summary.total_flows,
        summary.total_commands,
        summary.passed,
        summary.failed,
        summary.session_id,
        results.generated_at,
        pass_rate = pass_rate,
        charts_row_html = charts_row_html,
        stability_table_html = stability_table_html,
        flows_html = flows_html
    )
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn camera_hint_html(error: &str) -> String {
    let Some(hint) = crate::camera::launcher::camera_failure_hint(error) else {
        return String::new();
    };

    format!(
        r#"<div class="camera-hint"><strong>Camera next steps</strong><pre>{}</pre></div>"#,
        html_escape(&hint)
    )
}

/// Extracts the human-meaningful "target" (flow name, or file/suite name for a
/// directory run) out of a session id like `session_lumi_life_2026-08-26_18-13-21`
/// - strips the `session_` prefix and the trailing `_<date>_<time>` timestamp,
/// leaving just `lumi_life`. Shared by the sessions dashboard, the report title
/// default, and `summary_html`'s run-history section, all of which need to group
/// sessions by "which file/flow was this" rather than by the timestamped id.
pub(crate) fn target_name_from_session_id(session_id: &str, total_flows: usize) -> String {
    session_id
        .strip_prefix("session_")
        .and_then(|s| {
            let idx = s.rfind('_')?;
            let before_time = &s[..idx];
            let date_idx = before_time.rfind('_')?;
            let candidate_date = &before_time[date_idx + 1..];
            if candidate_date.len() == 10 && candidate_date.contains('-') {
                Some(before_time[..date_idx].to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| if total_flows == 1 { "flow".to_string() } else { "suite".to_string() })
}

pub(crate) fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let minutes = ms / 60000;
        let seconds = (ms % 60000) as f64 / 1000.0;
        format!("{}m {:.0}s", minutes, seconds)
    }
}

/// Renders a donut chart as a pure-CSS conic-gradient ring (no JS/canvas/SVG math,
/// so it always renders correctly in a standalone `file://` HTML report with no
/// external dependencies). `segments` is `(css_color, value)`; segments with value
/// <= 0 are skipped. Falls back to a flat neutral ring when everything is zero.
fn donut_chart_html(segments: &[(&str, f64)], center_value: &str, center_label: &str) -> String {
    let total: f64 = segments.iter().map(|(_, v)| v.max(0.0)).sum();
    let gradient = if total <= 0.0 {
        "conic-gradient(#1f2937 0% 100%)".to_string()
    } else {
        let mut stops = Vec::new();
        let mut cum = 0.0;
        for (color, value) in segments {
            if *value <= 0.0 {
                continue;
            }
            let start = cum / total * 100.0;
            cum += value;
            let end = cum / total * 100.0;
            stops.push(format!("{} {:.3}% {:.3}%", color, start, end));
        }
        if stops.is_empty() {
            "conic-gradient(#1f2937 0% 100%)".to_string()
        } else {
            format!("conic-gradient({})", stops.join(", "))
        }
    };
    format!(
        r#"<div class="donut" style="background: {gradient};">
            <div class="donut-hole">
                <div class="donut-value">{center_value}</div>
                <div class="donut-label">{center_label}</div>
            </div>
        </div>"#,
        gradient = gradient,
        center_value = center_value,
        center_label = html_escape(center_label)
    )
}

/// Legend list accompanying a `donut_chart_html` - `items` is `(css_color, label, value)`.
fn donut_legend_html(items: &[(&str, &str, f64)]) -> String {
    let mut html = String::new();
    for (color, label, value) in items {
        html.push_str(&format!(
            r#"<div class="legend-item"><span class="legend-dot" style="background:{color};"></span><span>{label}</span><span class="legend-count">{value}</span></div>"#,
            color = color,
            label = html_escape(label),
            value = *value as i64
        ));
    }
    html
}

/// Renders a simple vertical bar-per-run trend strip (e.g. pass rate history across
/// the most recent sessions of a flow), oldest-to-newest left-to-right. Pure CSS
/// (flex + height %), same rationale as the donut: must render standalone offline.
fn trend_bars_html(items: &[(String, f64, bool)]) -> String {
    let mut html = String::new();
    for (label, rate, passed) in items {
        let color = if *passed { "var(--green)" } else { "var(--red)" };
        let height = rate.clamp(0.0, 100.0).max(4.0);
        html.push_str(&format!(
            r#"<div class="trend-bar-wrap" title="{label}: {rate:.0}%"><div class="trend-bar" style="height:{h:.0}%; background:{color};"></div></div>"#,
            label = html_escape(label),
            rate = rate,
            h = height,
            color = color
        ));
    }
    html
}

/// Item summary for test sessions dashboard
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionDashboardItem {
    pub session_id: String,
    pub target_name: String,
    pub created_at: String,
    pub total_flows: usize,
    pub total_commands: usize,
    pub passed: usize,
    pub failed: usize,
    pub duration_ms: u64,
    pub report_path: String,
    pub is_passed: bool,
}

/// Generate Sessions History Dashboard HTML (output/index.html and output/sessions/index.html)
pub fn generate_sessions_dashboard(output_dir: &Path) -> Result<()> {
    let sessions_dir = output_dir.join("sessions");
    if !sessions_dir.exists() {
        return Ok(());
    }

    let mut sessions: Vec<SessionDashboardItem> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let session_path = entry.path();
            if !session_path.is_dir() {
                continue;
            }

            let session_id = entry.file_name().to_string_lossy().to_string();
            let session_info_path = session_path.join("session.json");

            if session_info_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&session_info_path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let summary = val.get("summary");
                        let total_flows = summary
                            .and_then(|s| s.get("totalFlows").or_else(|| s.get("total_flows")))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        let total_commands = summary
                            .and_then(|s| s.get("totalCommands").or_else(|| s.get("total_commands")))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        let passed = summary
                            .and_then(|s| s.get("passed"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        let failed = summary
                            .and_then(|s| s.get("failed"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        let duration_ms = summary
                            .and_then(|s| s.get("totalDurationMs").or_else(|| s.get("total_duration_ms")))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let created_at = val
                            .get("created_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let target_name = target_name_from_session_id(&session_id, total_flows);

                        let rel_report = format!("sessions/{}/report/summary.html", session_id);
                        let is_passed = failed == 0 && (passed > 0 || total_commands > 0);

                        sessions.push(SessionDashboardItem {
                            session_id,
                            target_name,
                            created_at,
                            total_flows,
                            total_commands,
                            passed,
                            failed,
                            duration_ms,
                            report_path: rel_report,
                            is_passed,
                        });
                    }
                }
            }
        }
    }

    sessions.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.session_id.cmp(&a.session_id))
    });

    let html = render_sessions_dashboard_html(&sessions);
    let index_path = output_dir.join("index.html");
    std::fs::write(&index_path, &html)?;

    Ok(())
}

fn render_sessions_dashboard_html(sessions: &[SessionDashboardItem]) -> String {
    let total_sessions = sessions.len();
    let passed_sessions = sessions.iter().filter(|s| s.is_passed).count();
    let failed_sessions = total_sessions.saturating_sub(passed_sessions);
    let pass_rate = if total_sessions > 0 {
        (passed_sessions as f64 / total_sessions as f64 * 100.0) as u32
    } else {
        0
    };
    // `sessions` arrives newest-first (see the sort in generate_sessions_dashboard),
    // so the first item is the most recent run - link straight to its report
    // instead of a separate top-level "report.html" (removed; summary.html per
    // session is now the only human-facing report).
    let latest_report_button = sessions
        .first()
        .map(|s| format!(r#"<a href="{}" class="btn-view" style="padding: 0.5rem 1rem; font-size: 0.875rem;">📊 Xem báo cáo mới nhất ↗</a>"#, s.report_path))
        .unwrap_or_default();

    let sessions_donut_html = donut_chart_html(
        &[
            ("#10b981", passed_sessions as f64),
            ("#ef4444", failed_sessions as f64),
        ],
        &format!("{}%", pass_rate),
        "Pass Rate",
    );
    let sessions_donut_legend = donut_legend_html(&[
        ("#10b981", "Passed", passed_sessions as f64),
        ("#ef4444", "Failed", failed_sessions as f64),
    ]);

    // Trend strip: pass rate per session, oldest-to-newest (left-to-right), capped
    // to the most recent 30 runs so it stays readable. `sessions` arrives newest-
    // first (see the sort in generate_sessions_dashboard), so take the head then
    // reverse for chronological order.
    const TREND_MAX_POINTS: usize = 30;
    let trend_points: Vec<(String, f64, bool)> = sessions
        .iter()
        .take(TREND_MAX_POINTS)
        .rev()
        .map(|s| {
            let total_cmds = s.passed + s.failed;
            let rate = if total_cmds > 0 { s.passed as f64 / total_cmds as f64 * 100.0 } else { 0.0 };
            (s.created_at.clone(), rate, s.is_passed)
        })
        .collect();
    let trend_chart_html = if trend_points.len() > 1 {
        format!(
            r#"<div class="chart-card" style="flex: 1.6;">
                <h3>📈 Pass Rate Trend (last {} runs)</h3>
                <div class="trend-chart">{}</div>
            </div>"#,
            trend_points.len(),
            trend_bars_html(&trend_points)
        )
    } else {
        String::new()
    };

    let overview_charts_html = format!(
        r#"<div class="charts-row">
            <div class="chart-card">
                <h3>✅ Session Outcomes</h3>
                <div class="donut-wrap">
                    {donut}
                    <div class="legend">{legend}</div>
                </div>
            </div>
            {trend}
        </div>"#,
        donut = sessions_donut_html,
        legend = sessions_donut_legend,
        trend = trend_chart_html
    );

    // Calculate aggregated stability per flow/target across all sessions
    let mut flow_stats: std::collections::HashMap<String, (usize, usize, usize, u64)> = std::collections::HashMap::new();
    for item in sessions {
        let entry = flow_stats.entry(item.target_name.clone()).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        if item.is_passed {
            entry.1 += 1;
        } else {
            entry.2 += 1;
        }
        entry.3 += item.duration_ms;
    }

    let mut stability_section_html = String::new();
    if !flow_stats.is_empty() {
        let mut rows = String::new();
        let mut sorted: Vec<_> = flow_stats.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        for (name, (total, passed, failed, duration_ms)) in sorted {
            let rate = if total > 0 { (passed as f64 / total as f64 * 100.0) as u32 } else { 0 };
            let (status_badge, status_class) = if failed == 0 {
                ("STABLE (100%)", "passed")
            } else if passed > 0 {
                ("FLAKY", "flaky")
            } else {
                ("FAILING", "failed")
            };

            rows.push_str(&format!(
                r#"
                <tr>
                    <td><span class="flow-pill">{}</span></td>
                    <td><strong>{}</strong> session(s)</td>
                    <td><span style="color: var(--green); font-weight: 600;">{} passed</span> / <span style="color: var(--red); font-weight: 600;">{} failed</span></td>
                    <td><strong>{}%</strong></td>
                    <td>{}</td>
                    <td><span class="status-badge {}">{}</span></td>
                </tr>
                "#,
                html_escape(&name),
                total,
                passed,
                failed,
                rate,
                format_duration(duration_ms),
                status_class,
                status_badge
            ));
        }

        stability_section_html = format!(
            r#"
            <div class="table-card" style="margin-bottom: 2rem;">
                <div style="padding: 1rem 1.25rem; background: var(--glass); border-bottom: 1px solid var(--border); font-weight: 700;">
                    📈 Flow Reliability & Flakiness Breakdown
                </div>
                <table>
                    <thead>
                        <tr>
                            <th>Flow / Target</th>
                            <th>Total Sessions</th>
                            <th>Passed / Failed</th>
                            <th>Pass Rate</th>
                            <th>Total Duration</th>
                            <th>Reliability Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
            "#,
            rows
        );
    }

    let mut rows_html = String::new();
    for item in sessions {
        let (status_badge, status_class) = if item.is_passed {
            ("PASSED", "passed")
        } else {
            ("FAILED", "failed")
        };

        let duration_text = format_duration(item.duration_ms);
        // `created_at` is "%Y-%m-%d %H:%M:%S" - the leading 10 chars are the date,
        // used for the date-range filter (string comparison works fine on this format).
        let date_only = item.created_at.get(..10).unwrap_or(&item.created_at);

        rows_html.push_str(&format!(
            r##"
            <tr class="session-row {status_class}" data-status="{status_class}" data-date="{date_only}" data-search="{search_text}">
                <td><span class="status-badge {status_class}">{status_badge}</span></td>
                <td class="target-name">
                    <span class="flow-pill">{target_name}</span>
                    <div class="session-id">{session_id}</div>
                </td>
                <td class="date-cell">{created_at}</td>
                <td>{total_flows} flow(s), {total_commands} cmd(s)</td>
                <td>{passed} pass / {failed} fail</td>
                <td class="duration-cell">{duration_text}</td>
                <td class="action-cell">
                    <a href="{link_path}" class="btn-view">View Report ↗</a>
                </td>
            </tr>
            "##,
            status_class = status_class,
            status_badge = status_badge,
            target_name = html_escape(&item.target_name),
            session_id = html_escape(&item.session_id),
            created_at = html_escape(&item.created_at),
            date_only = html_escape(date_only),
            total_flows = item.total_flows,
            total_commands = item.total_commands,
            passed = item.passed,
            failed = item.failed,
            duration_text = duration_text,
            link_path = item.report_path,
            search_text = html_escape(&format!("{} {} {}", item.session_id, item.target_name, item.created_at).to_lowercase())
        ));
    }

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Lumi Tester - Test Sessions Dashboard</title>
    <style>
        :root {{
            --bg-primary: #eef2f6;
            --bg-secondary: #ffffff;
            --text-primary: #1f2937;
            --text-secondary: #64748b;
            --border: #d7dee8;
            --green: #16a34a;
            --red: #dc2626;
            --yellow: #f59e0b;
            --purple: #0759b8;
            --blue: #0759b8;
            --glass: rgba(15, 23, 42, 0.035);
        }}
        * {{ margin: 0; padding: 0; box-sizing: border-box; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif; }}
        body {{ background-color: var(--bg-primary); color: var(--text-primary); padding: 2rem; min-height: 100vh; }}
        .container {{ max-width: 1200px; margin: 0 auto; }}
        header {{ display: flex; justify-content: space-between; align-items: center; margin-bottom: 2rem; }}
        .logo-tag {{ font-size: 0.8125rem; font-weight: 700; color: var(--purple); text-transform: uppercase; letter-spacing: 0.1em; margin-bottom: 0.25rem; }}
        h1 {{ font-size: 2rem; font-weight: 800; letter-spacing: -0.025em; }}
        
        .summary {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1.25rem; margin-bottom: 2rem; }}
        .stat {{ background: var(--bg-secondary); border: 1px solid var(--border); padding: 1.25rem; border-radius: 0.875rem; }}
        .stat-value {{ font-size: 2rem; font-weight: 800; margin-bottom: 0.25rem; }}
        .stat-label {{ color: var(--text-secondary); font-size: 0.8125rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; }}
        .stat.passed .stat-value {{ color: var(--green); }}
        .stat.failed .stat-value {{ color: var(--red); }}

        .charts-row {{ display: flex; gap: 1.5rem; margin-bottom: 2rem; flex-wrap: wrap; }}
        .chart-card {{ background: var(--bg-secondary); border: 1px solid var(--border); border-radius: 1rem; padding: 1.5rem; flex: 1; min-width: 280px; box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1); }}
        .chart-card h3 {{ font-size: 0.8125rem; font-weight: 700; margin-bottom: 1.25rem; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em; }}
        .donut-wrap {{ display: flex; align-items: center; gap: 1.75rem; flex-wrap: wrap; }}
        .donut {{ width: 140px; height: 140px; border-radius: 50%; flex-shrink: 0; display: flex; align-items: center; justify-content: center; }}
        .donut-hole {{ width: 68%; height: 68%; border-radius: 50%; background: var(--bg-secondary); display: flex; flex-direction: column; align-items: center; justify-content: center; }}
        .donut-value {{ font-size: 1.5rem; font-weight: 800; }}
        .donut-label {{ font-size: 0.6875rem; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em; margin-top: 0.15rem; }}
        .legend {{ display: flex; flex-direction: column; gap: 0.6rem; min-width: 120px; }}
        .legend-item {{ display: flex; align-items: center; gap: 0.5rem; font-size: 0.8125rem; }}
        .legend-dot {{ width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0; }}
        .legend-count {{ margin-left: auto; font-weight: 700; font-family: monospace; }}
        .trend-chart {{ display: flex; align-items: flex-end; gap: 4px; height: 90px; }}
        .trend-bar-wrap {{ flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: flex-end; height: 100%; min-width: 5px; }}
        .trend-bar {{ width: 100%; border-radius: 3px 3px 0 0; min-height: 3px; transition: opacity 0.15s; }}
        .trend-bar-wrap:hover .trend-bar {{ opacity: 0.7; }}

        .toolbar {{ display: flex; justify-content: space-between; align-items: center; gap: 1rem; margin-bottom: 1.5rem; flex-wrap: wrap; }}
        .filter-group {{ display: flex; gap: 0.5rem; }}
        .filter-btn {{ background: var(--bg-secondary); border: 1px solid var(--border); color: var(--text-secondary); padding: 0.5rem 1rem; border-radius: 0.5rem; cursor: pointer; font-weight: 600; font-size: 0.875rem; transition: all 0.2s; }}
        .filter-btn.active {{ background: var(--purple); color: #fff; border-color: var(--purple); }}
        .search-box {{ flex: 1; max-width: 380px; position: relative; }}
        .search-box input {{ width: 100%; background: var(--bg-secondary); border: 1px solid var(--border); color: var(--text-primary); padding: 0.5rem 1rem; border-radius: 0.5rem; font-size: 0.875rem; outline: none; }}
        .search-box input:focus {{ border-color: var(--purple); }}
        .date-range {{ display: flex; align-items: center; gap: 0.5rem; }}
        .date-range label {{ font-size: 0.8125rem; color: var(--text-secondary); font-weight: 600; }}
        .date-range input[type="date"] {{ background: var(--bg-secondary); border: 1px solid var(--border); color: var(--text-primary); padding: 0.4rem 0.6rem; border-radius: 0.5rem; font-size: 0.8125rem; outline: none; }}
        .date-range input[type="date"]:focus {{ border-color: var(--purple); }}

        .table-card {{ background: var(--bg-secondary); border: 1px solid var(--border); border-radius: 1rem; overflow: hidden; box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1); }}
        table {{ width: 100%; border-collapse: collapse; text-align: left; }}
        th {{ padding: 1rem 1.25rem; font-size: 0.75rem; font-weight: 700; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em; border-bottom: 1px solid var(--border); background: var(--glass); }}
        td {{ padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); font-size: 0.875rem; }}
        tr:last-child td {{ border-bottom: none; }}
        tr.session-row:hover {{ background: rgba(7, 89, 184, 0.05); }}

        .status-badge {{ display: inline-block; padding: 0.25rem 0.625rem; border-radius: 9999px; font-size: 0.75rem; font-weight: 700; text-transform: uppercase; }}
        .status-badge.passed {{ background: rgba(16, 185, 129, 0.15); color: var(--green); }}
        .status-badge.failed {{ background: rgba(239, 68, 68, 0.15); color: var(--red); }}

        .flow-pill {{ display: inline-block; background: rgba(7, 89, 184, 0.1); color: var(--purple); padding: 0.2rem 0.5rem; border-radius: 0.375rem; font-weight: 600; font-size: 0.8125rem; margin-bottom: 0.2rem; }}
        .session-id {{ font-size: 0.75rem; color: var(--text-secondary); font-family: monospace; }}
        .date-cell {{ color: var(--text-secondary); font-size: 0.8125rem; }}
        .duration-cell {{ font-weight: 600; color: #334155; }}
        
        .btn-view {{ display: inline-block; background: var(--purple); color: #fff; text-decoration: none; padding: 0.375rem 0.75rem; border-radius: 0.375rem; font-weight: 600; font-size: 0.8125rem; transition: opacity 0.2s; }}
        .btn-view:hover {{ opacity: 0.85; }}

        .empty-state {{ text-align: center; padding: 3rem; color: var(--text-secondary); }}
    </style>
</head>
<body>
    <div class="container">
        <header>
            <div>
                <div class="logo-tag">Lumi Tester</div>
                <h1>Test Sessions Dashboard</h1>
            </div>
            <div>
                {latest_report_button}
            </div>
        </header>

        <div class="summary">
            <div class="stat">
                <div class="stat-value">{total_sessions}</div>
                <div class="stat-label">Total Sessions</div>
            </div>
            <div class="stat passed">
                <div class="stat-value">{passed_sessions}</div>
                <div class="stat-label">Passed</div>
            </div>
            <div class="stat failed">
                <div class="stat-value">{failed_sessions}</div>
                <div class="stat-label">Failed</div>
            </div>
            <div class="stat">
                <div class="stat-value" style="color: var(--purple);">{pass_rate}%</div>
                <div class="stat-label">Success Rate</div>
            </div>
        </div>

        {overview_charts_html}

        {stability_section_html}

        <div class="toolbar">
            <div class="filter-group">
                <button class="filter-btn active" onclick="setFilter('all', this)">All ({total_sessions})</button>
                <button class="filter-btn" onclick="setFilter('passed', this)">Passed ({passed_sessions})</button>
                <button class="filter-btn" onclick="setFilter('failed', this)">Failed ({failed_sessions})</button>
            </div>
            <div class="date-range">
                <label for="dateFrom">Từ</label>
                <input type="date" id="dateFrom" onchange="filterRows()">
                <label for="dateTo">Đến</label>
                <input type="date" id="dateTo" onchange="filterRows()">
                <button class="filter-btn" onclick="clearDateRange()" title="Xoá bộ lọc ngày">✕</button>
            </div>
            <div class="search-box">
                <input type="text" id="searchInput" placeholder="Search by session name, flow, date..." onkeyup="filterRows()">
            </div>
        </div>

        <div class="table-card">
            <table>
                <thead>
                    <tr>
                        <th>Status</th>
                        <th>Target & Session ID</th>
                        <th>Executed At</th>
                        <th>Execution Counts</th>
                        <th>Results</th>
                        <th>Duration</th>
                        <th>Action</th>
                    </tr>
                </thead>
                <tbody id="sessionTableBody">
                    {rows_html}
                </tbody>
            </table>
        </div>
    </div>

    <script>
        let currentFilter = 'all';

        function setFilter(status, btn) {{
            currentFilter = status;
            document.querySelectorAll('.filter-btn').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            filterRows();
        }}

        function clearDateRange() {{
            document.getElementById('dateFrom').value = '';
            document.getElementById('dateTo').value = '';
            filterRows();
        }}

        function filterRows() {{
            const search = document.getElementById('searchInput').value.toLowerCase();
            const dateFrom = document.getElementById('dateFrom').value;
            const dateTo = document.getElementById('dateTo').value;
            const rows = document.querySelectorAll('.session-row');

            rows.forEach(row => {{
                const status = row.getAttribute('data-status');
                const text = row.getAttribute('data-search');
                const date = row.getAttribute('data-date');

                const matchesFilter = (currentFilter === 'all' || status === currentFilter);
                const matchesSearch = text.includes(search);
                const matchesFrom = !dateFrom || date >= dateFrom;
                const matchesTo = !dateTo || date <= dateTo;

                if (matchesFilter && matchesSearch && matchesFrom && matchesTo) {{
                    row.style.display = '';
                }} else {{
                    row.style.display = 'none';
                }}
            }});
        }}
    </script>
</body>
</html>"##,
        latest_report_button = latest_report_button,
        total_sessions = total_sessions,
        passed_sessions = passed_sessions,
        failed_sessions = failed_sessions,
        pass_rate = pass_rate,
        overview_charts_html = overview_charts_html,
        stability_section_html = stability_section_html,
        rows_html = rows_html
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_screenshot_is_thumbnailed_but_keeps_original_dimensions_for_bbox_math() {
        use crate::runner::state::{CommandStateReport, CommandStatus, FlowStateReport, FlowStatus, TestSummary};
        use base64::{engine::general_purpose::STANDARD, Engine};
        use image::{ImageBuffer, Rgb};

        // A large, phone-resolution-like screenshot - big enough that embedding
        // it raw would meaningfully bloat the report, and specific enough
        // (1440x2960) that we can assert the exact original dims survive into
        // the HTML even though the embedded copy is downscaled.
        let dir = std::env::temp_dir().join(format!("lumi_html_shot_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let shot_path = dir.join("fail_shot.png");
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(1440, 2960, |x, y| {
            if (x / 40 + y / 40) % 2 == 0 {
                Rgb([250, 250, 250])
            } else {
                Rgb([15, 15, 15])
            }
        });
        img.save(&shot_path).expect("failed to write test screenshot");
        let raw_bytes = std::fs::read(&shot_path).unwrap();
        let raw_b64_len = STANDARD.encode(&raw_bytes).len();

        let results = TestResults {
            session_id: "session_test".to_string(),
            generated_at: "2026-09-11 00:00:00".to_string(),
            multi_target: None,
            summary: TestSummary {
                session_id: "session_test".to_string(),
                total_flows: 1,
                total_commands: 1,
                passed: 0,
                failed: 1,
                skipped: 0,
                total_duration_ms: Some(100),
            },
            flows: vec![FlowStateReport {
                flow_name: "flow".to_string(),
                flow_path: "flow.yaml".to_string(),
                status: FlowStatus::Failed,
                total_duration_ms: Some(100),
                error: Some("boom".to_string()),
                video_path: None,
                commands: vec![CommandStateReport {
                    index: 0,
                    command_name: "see".to_string(),
                    command_display: "see(\"X\")".to_string(),
                    status: CommandStatus::Failed { error: "boom".to_string() },
                    duration_ms: Some(50),
                    screenshot_path: Some(shot_path.display().to_string()),
                    ui_hierarchy_path: None,
                    log_path: None,
                    retry_count: 0,
                }],
            }],
        };

        let html = generate_html(&results);

        // Original resolution is threaded through as arguments to
        // `showScreenshot(...)` for the bbox-overlay math, even though the
        // embedded image itself is downscaled.
        assert!(
            html.contains("showScreenshot(this.querySelector('img').src, this.getAttribute('data-elements'), 1440, 2960)"),
            "original dimensions not passed through to showScreenshot()"
        );
        assert!(html.contains("data:image/webp;base64,"), "expected an embedded WebP data URI");

        // The embedded copy must actually be smaller than the raw file would
        // have been - this is the whole point of thumbnailing before embedding.
        let embedded_b64_len = html
            .split("data:image/webp;base64,")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("could not find embedded image data URI")
            .len();
        assert!(
            embedded_b64_len < raw_b64_len,
            "embedded thumbnail ({embedded_b64_len} b64 chars) should be smaller than the raw screenshot ({raw_b64_len} b64 chars)"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn camera_failure_hint_is_rendered_as_html() {
        let error = "device check failed: button 'device_2.button_1' is 'UNKNOWN', expected 'BLUE'\ncamera evidence: output/camera_evidence/default_123";

        let html = camera_hint_html(error);

        assert!(html.contains("Camera next steps"));
        assert!(html.contains("lumi-tester camera profile"));
        assert!(html.contains("device_2.button_1"));
    }

    #[test]
    fn test_render_sessions_dashboard_html() {
        let items = vec![
            SessionDashboardItem {
                session_id: "session_slider_2026-08-22_10-04-45".to_string(),
                target_name: "slider".to_string(),
                created_at: "2026-08-22 10:04:45".to_string(),
                total_flows: 1,
                total_commands: 3,
                passed: 3,
                failed: 0,
                duration_ms: 1500,
                report_path: "sessions/session_slider_2026-08-22_10-04-45/report/summary.html".to_string(),
                is_passed: true,
            },
            SessionDashboardItem {
                session_id: "session_login_2026-08-22_09-15-30".to_string(),
                target_name: "login".to_string(),
                created_at: "2026-08-22 09:15:30".to_string(),
                total_flows: 1,
                total_commands: 5,
                passed: 4,
                failed: 1,
                duration_ms: 2400,
                report_path: "sessions/session_login_2026-08-22_09-15-30/report/summary.html".to_string(),
                is_passed: false,
            },
        ];

        let html = render_sessions_dashboard_html(&items);
        assert!(html.contains("Lumi Tester - Test Sessions Dashboard"));
        assert!(html.contains("slider"));
        assert!(html.contains("login"));
        assert!(html.contains("PASSED"));
        assert!(html.contains("FAILED"));
    }

    #[test]
    fn test_generate_sessions_dashboard_from_dir() {
        let temp_dir = std::env::temp_dir().join(format!("lumi_dash_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
        let sessions_dir = temp_dir.join("sessions");
        let session1_dir = sessions_dir.join("session_login_2026-08-22_10-00-00");
        std::fs::create_dir_all(&session1_dir).unwrap();

        let session_json = serde_json::json!({
            "created_at": "2026-08-22 10:00:00",
            "session_id": "session_login_2026-08-22_10-00-00",
            "summary": {
                "failed": 0,
                "passed": 5,
                "sessionId": "session_login_2026-08-22_10-00-00",
                "skipped": 0,
                "totalCommands": 5,
                "totalDurationMs": 1200,
                "totalFlows": 1
            }
        });
        std::fs::write(session1_dir.join("session.json"), session_json.to_string()).unwrap();

        assert!(generate_sessions_dashboard(&temp_dir).is_ok());

        let index_content = std::fs::read_to_string(temp_dir.join("index.html")).unwrap();
        assert!(index_content.contains("PASSED"));
        assert!(index_content.contains("5 cmd(s)"));
        assert!(index_content.contains("5 pass / 0 fail"));
        assert!(index_content.contains("1.2s"));
        assert!(index_content.contains("report/summary.html"));
        assert!(!temp_dir.join("sessions/index.html").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_extract_ui_elements_multi_platform() {
        // 1. Android XML
        let android_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <hierarchy rotation="0">
            <node text="Login" resource-id="com.example:id/login_btn" class="android.widget.Button" package="com.example" content-desc="" bounds="[100,200][300,280]" clickable="true" enabled="true"/>
        </hierarchy>"#;
        let android_boxes = extract_ui_elements(android_xml);
        assert_eq!(android_boxes.len(), 1);
        assert_eq!(android_boxes[0].left, 100);
        assert_eq!(android_boxes[0].top, 200);
        assert_eq!(android_boxes[0].right, 300);
        assert_eq!(android_boxes[0].bottom, 280);
        assert_eq!(android_boxes[0].text, "Login");
        assert_eq!(android_boxes[0].id, "com.example:id/login_btn");
        assert!(android_boxes[0].clickable);

        // 2. iOS JSON
        let ios_json = r#"[
            {
                "AXLabel": "Submit",
                "AXUniqueId": "submit_btn",
                "type": "Button",
                "frame": { "x": 50.0, "y": 150.0, "width": 200.0, "height": 44.0 },
                "enabled": true,
                "visible": true
            }
        ]"#;
        let ios_boxes = extract_ui_elements(ios_json);
        assert_eq!(ios_boxes.len(), 1);
        assert_eq!(ios_boxes[0].left, 50);
        assert_eq!(ios_boxes[0].top, 150);
        assert_eq!(ios_boxes[0].right, 250);
        assert_eq!(ios_boxes[0].bottom, 194);
        assert_eq!(ios_boxes[0].text, "Submit");
        assert_eq!(ios_boxes[0].id, "submit_btn");
        assert!(ios_boxes[0].clickable);

        // 3. macOS XML
        let macos_xml = r#"<hierarchy platform="macos">
            <element role="AXButton" title="Save" description="Save document" value="" placeholder="" id="save_action" x="80" y="120" width="100" height="30"/>
        </hierarchy>"#;
        let macos_boxes = extract_ui_elements(macos_xml);
        assert_eq!(macos_boxes.len(), 1);
        assert_eq!(macos_boxes[0].left, 80);
        assert_eq!(macos_boxes[0].top, 120);
        assert_eq!(macos_boxes[0].right, 180);
        assert_eq!(macos_boxes[0].bottom, 150);
        assert_eq!(macos_boxes[0].text, "Save");
        assert_eq!(macos_boxes[0].id, "save_action");
        assert!(macos_boxes[0].clickable);

        // 4. Windows XML
        let win_xml = r#"<hierarchy platform="windows">
            <element type="Button" name="OK" id="ok_btn" class="Button" description="" offscreen="false" x="200" y="400" width="120" height="40"/>
        </hierarchy>"#;
        let win_boxes = extract_ui_elements(win_xml);
        assert_eq!(win_boxes.len(), 1);
        assert_eq!(win_boxes[0].left, 200);
        assert_eq!(win_boxes[0].top, 400);
        assert_eq!(win_boxes[0].right, 320);
        assert_eq!(win_boxes[0].bottom, 440);
        assert_eq!(win_boxes[0].text, "OK");
        assert_eq!(win_boxes[0].id, "ok_btn");

        // 5. Web XML
        let web_xml = r#"<hierarchy platform="web">
            <element tag="button" id="submit_form" class="btn btn-primary" text="Checkout" x="300" y="500" width="150" height="45" clickable="true"/>
        </hierarchy>"#;
        let web_boxes = extract_ui_elements(web_xml);
        assert_eq!(web_boxes.len(), 1);
        assert_eq!(web_boxes[0].left, 300);
        assert_eq!(web_boxes[0].top, 500);
        assert_eq!(web_boxes[0].right, 450);
        assert_eq!(web_boxes[0].bottom, 545);
        assert_eq!(web_boxes[0].text, "Checkout");
        assert_eq!(web_boxes[0].id, "submit_form");
        assert!(web_boxes[0].clickable);
    }
}

