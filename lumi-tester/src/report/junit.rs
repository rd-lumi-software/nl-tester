use super::types::TestResults;
use crate::runner::state::{FlowStateReport, FlowStatus};
use anyhow::Result;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::Writer;
use std::io::Cursor;
use std::path::Path;

/// Generate JUnit XML report string from TestResults
pub fn generate_junit_xml(results: &TestResults) -> Result<String> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // Write XML declaration
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    // Calculate totals
    let total_tests = results.flows.len();
    let failures = results
        .flows
        .iter()
        .filter(|f| {
            matches!(
                f.status,
                FlowStatus::Failed | FlowStatus::PartiallyPassed { .. }
            )
        })
        .count();
    let skipped = 0;
    let total_duration: u64 = results
        .flows
        .iter()
        .map(|f| f.total_duration_ms.unwrap_or(0))
        .sum();

    // <testsuites>
    let mut suites_start = BytesStart::new("testsuites");
    suites_start.push_attribute(("name", "lumi-tester-run"));
    suites_start.push_attribute(("tests", total_tests.to_string().as_str()));
    suites_start.push_attribute(("failures", failures.to_string().as_str()));
    suites_start.push_attribute(("skipped", skipped.to_string().as_str()));
    suites_start.push_attribute((
        "time",
        (total_duration as f64 / 1000.0).to_string().as_str(),
    ));
    writer.write_event(Event::Start(suites_start))?;

    // Single <testsuite> for this run (since we run a set of flows)
    // In a more complex setup, we might group by directory or tag
    let mut suite_start = BytesStart::new("testsuite");
    suite_start.push_attribute(("name", "default"));
    suite_start.push_attribute(("tests", total_tests.to_string().as_str()));
    suite_start.push_attribute(("failures", failures.to_string().as_str()));
    suite_start.push_attribute(("skipped", skipped.to_string().as_str()));
    suite_start.push_attribute(("id", results.session_id.as_str()));
    suite_start.push_attribute((
        "time",
        (total_duration as f64 / 1000.0).to_string().as_str(),
    ));
    suite_start.push_attribute(("timestamp", results.generated_at.as_str()));
    writer.write_event(Event::Start(suite_start))?;

    for flow in &results.flows {
        write_test_case(&mut writer, flow)?;
    }

    writer.write_event(Event::End(BytesEnd::new("testsuite")))?;
    writer.write_event(Event::End(BytesEnd::new("testsuites")))?;

    let result = writer.into_inner().into_inner();
    let xml = String::from_utf8(result)?;
    Ok(xml)
}

fn write_test_case<W: std::io::Write>(
    writer: &mut Writer<W>,
    flow: &FlowStateReport,
) -> Result<()> {
    let mut case_start = BytesStart::new("testcase");
    // Classname is usually package.class, here we can use the file path or directory
    let classname = flow.flow_path.replace("/", ".");

    case_start.push_attribute(("name", flow.flow_name.as_str()));
    case_start.push_attribute(("classname", classname.as_str()));
    case_start.push_attribute((
        "time",
        (flow.total_duration_ms.unwrap_or(0) as f64 / 1000.0)
            .to_string()
            .as_str(),
    ));

    writer.write_event(Event::Start(case_start))?;

    match flow.status {
        FlowStatus::Failed | FlowStatus::PartiallyPassed { .. } => {
            let mut fail_start = BytesStart::new("failure");
            fail_start
                .push_attribute(("message", flow.error.as_deref().unwrap_or("Unknown error")));
            fail_start.push_attribute(("type", "AssertionError"));
            writer.write_event(Event::Start(fail_start))?;

            if let Some(err) = &flow.error {
                writer.write_event(Event::Text(quick_xml::events::BytesText::new(err)))?;
            }

            writer.write_event(Event::End(BytesEnd::new("failure")))?;
        }
        _ => {}
    }

    // Add system-out for logs if needed? JUnit usually puts logs in system-out
    // We could format commands history here but it might be too verbose.
    // For now, let's keep it clean.

    writer.write_event(Event::End(BytesEnd::new("testcase")))?;
    Ok(())
}

/// Write report to file
pub fn write_report(results: &TestResults, output_dir: &Path) -> Result<()> {
    let xml = generate_junit_xml(results)?;
    let path = output_dir.join("junit.xml");
    std::fs::write(&path, xml)?;
    println!("    Generated JUnit report: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::types::TestResults;
    use crate::runner::state::{FlowStateReport, FlowStatus, TestSummary};

    #[test]
    fn test_generate_junit_xml() {
        let results = TestResults {
            session_id: "test-session".to_string(),
            flows: vec![
                FlowStateReport {
                    flow_name: "Login Flow".to_string(),
                    flow_path: "flows/login.yaml".to_string(),
                    status: FlowStatus::Passed,
                    total_duration_ms: Some(1500),
                    error: None,
                    commands: vec![],
                    video_path: None,
                },
                FlowStateReport {
                    flow_name: "Checkout Flow".to_string(),
                    flow_path: "flows/checkout.yaml".to_string(),
                    status: FlowStatus::Failed,
                    total_duration_ms: Some(2000),
                    error: Some("Element not found".to_string()),
                    commands: vec![],
                    video_path: None,
                },
            ],
            summary: TestSummary {
                session_id: "test-session".to_string(),
                total_flows: 2,
                total_commands: 10,
                passed: 9,
                failed: 1,
                skipped: 0,
                total_duration_ms: Some(3500),
            },
            generated_at: "2023-01-01 12:00:00".to_string(),
            multi_target: None,
        };

        let xml = generate_junit_xml(&results).expect("Failed to generate XML");

        assert!(xml.contains(r#"<testsuites name="lumi-tester-run""#));
        assert!(xml.contains(r#"tests="2""#));
        assert!(xml.contains(r#"failures="1""#));
        assert!(xml.contains(r#"<testcase name="Login Flow""#));
        assert!(xml.contains(r#"message="Element not found""#));
    }
}
