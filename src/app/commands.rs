//! Device/health reports and the bounded proxy command.
//! Library modules collect observations and run sessions; this layer chooses
//! each command's reporting and exit policy.

use std::path::Path;

use edgepad::device::{discover_device_report, format_device_line, touchpad_candidates};
use edgepad::doctor::{run_doctor, DoctorCheck, DoctorConfig, DoctorReport, DoctorSection};
use edgepad::proxy::{run_proxy, ProxyMode, ProxyRunConfig, ProxyRunLimit, StopAfterFrameLimit};
use edgepad::status::{run_status, StatusConfig, StatusReport};

use super::cli::ProxyArgs;
use super::presentation::{print_proxy_summary, print_recognition_profile};
use super::profile::load_recognition_profile;

pub(super) fn devices(root: &Path, show_all: bool) -> Result<(), String> {
    let report = discover_device_report(root)
        .map_err(|err| format!("failed to list {}: {err}", root.display()))?;

    if report.event_node_count == 0 {
        println!("no event devices found under {}", root.display());
        return Ok(());
    }

    if report.summaries.is_empty() {
        println!(
            "no readable event devices found under {} ({}; try sudo, group input, or seat/logind ACLs)",
            root.display(),
            event_node_count_text(report.event_node_count)
        );
        return Ok(());
    }

    if show_all {
        for summary in &report.summaries {
            println!("{}", format_device_line(summary));
        }
        return Ok(());
    }

    let candidates = touchpad_candidates(&report.summaries);
    if candidates.is_empty() {
        println!("no touchpad candidates found under {}", root.display());
        println!(
            "readable non-touchpad devices: {} (use --all to inspect)",
            report.summaries.len()
        );
        return Ok(());
    }

    for summary in candidates {
        println!("{}", format_device_line(summary));
    }

    Ok(())
}

pub(super) fn doctor(config: &DoctorConfig) -> Result<i32, String> {
    let report = run_doctor(config);
    print_doctor_report(&report);
    if report.has_failures() {
        Ok(2)
    } else {
        Ok(0)
    }
}

pub(super) fn status(config: &StatusConfig) -> Result<i32, String> {
    let report = run_status(config);
    print_status_report(&report);
    Ok(report.result.exit_code())
}

fn print_status_report(report: &StatusReport) {
    println!("edgepad status");
    println!();

    for line in &report.lines {
        println!("{:<8} {}", line.subject.label(), line.detail);
    }

    println!();
    println!("{:<8} {}", "Result", report.result.label());
}

fn print_doctor_report(report: &DoctorReport) {
    println!("edgepad doctor");

    for section in DoctorSection::ALL {
        let checks = report
            .checks
            .iter()
            .filter(|check| check.section == section)
            .collect::<Vec<_>>();
        if checks.is_empty() {
            continue;
        }

        println!();
        println!("{}", section.title());
        for check in checks {
            print_doctor_check(check);
        }
    }

    let counts = report.counts();
    println!();
    println!("Summary");
    println!(
        "  ok={} warn={} fail={}",
        counts.ok, counts.warn, counts.fail
    );
    println!("  result: {}", doctor_result_label(report));
}

fn print_doctor_check(check: &DoctorCheck) {
    let mut detail_lines = check.detail.lines();
    let first_detail = detail_lines.next().unwrap_or("");
    println!(
        "  {:<5} {:<10} {}",
        check.status.label(),
        check.label,
        first_detail
    );
    for line in detail_lines {
        println!("  {:<5} {:<10} {}", "", "", line);
    }
}

fn doctor_result_label(report: &DoctorReport) -> &'static str {
    let counts = report.counts();
    if counts.fail > 0 {
        "problems found"
    } else if counts.warn > 0 {
        "usable with warnings"
    } else {
        "ready"
    }
}

fn event_node_count_text(count: usize) -> String {
    if count == 1 {
        "1 event node was present but could not be opened".to_string()
    } else {
        format!("{count} event nodes were present but could not be opened")
    }
}

pub(super) fn proxy(args: &ProxyArgs) -> Result<(), String> {
    let mut profile = load_recognition_profile(&args.profile)?;
    if let Some(edge_width) = args.edge_width_override {
        profile.override_edge_width(edge_width);
    }
    let device = args
        .device_override
        .as_ref()
        .or(profile.device.as_ref())
        .ok_or_else(|| "proxy --built-in-defaults requires --device auto|<event-node>".to_string())?
        .resolve(&args.input_root)?;

    print_recognition_profile(&profile);
    let summary = run_proxy(&ProxyRunConfig {
        device_path: device,
        limit: ProxyRunLimit::Frames {
            frame_boundaries: args.frames,
            stop_after_limit: match args.mode() {
                ProxyMode::DryRun => StopAfterFrameLimit::Immediately,
                ProxyMode::UinputGrab => StopAfterFrameLimit::WhenIdle,
            },
        },
        edge_widths: profile.edge_widths,
        engine_options: profile.engine_options,
        slider_specs: profile.slider_specs,
        mode: args.mode(),
    })?;
    print_proxy_summary(&summary);
    Ok(())
}
