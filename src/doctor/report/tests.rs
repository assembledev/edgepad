use super::*;

#[test]
fn report_counts_failures_warnings_and_successes() {
    let report = DoctorReport {
        checks: vec![
            DoctorCheck::new(CheckStatus::Ok, DoctorSection::Config, "a", ""),
            DoctorCheck::new(CheckStatus::Warn, DoctorSection::Config, "b", ""),
            DoctorCheck::new(CheckStatus::Fail, DoctorSection::Config, "c", ""),
        ],
    };

    assert!(report.has_failures());
    assert_eq!(
        report.counts(),
        DoctorCounts {
            ok: 1,
            warn: 1,
            fail: 1
        }
    );
}
