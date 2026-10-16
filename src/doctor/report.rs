#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheck {
    pub status: CheckStatus,
    pub section: DoctorSection,
    pub label: &'static str,
    pub detail: String,
}

impl DoctorCheck {
    pub(super) fn new(
        status: CheckStatus,
        section: DoctorSection,
        label: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status,
            section,
            label,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorSection {
    Config,
    Actions,
    Device,
    Access,
    Session,
    Service,
}

impl DoctorSection {
    pub const ALL: [Self; 6] = [
        Self::Config,
        Self::Actions,
        Self::Device,
        Self::Access,
        Self::Session,
        Self::Service,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Config => "Config",
            Self::Actions => "Actions",
            Self::Device => "Device",
            Self::Access => "Access",
            Self::Session => "Session",
            Self::Service => "Service",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }

    pub fn counts(&self) -> DoctorCounts {
        let mut counts = DoctorCounts::default();
        for check in &self.checks {
            match check.status {
                CheckStatus::Ok => counts.ok += 1,
                CheckStatus::Warn => counts.warn += 1,
                CheckStatus::Fail => counts.fail += 1,
            }
        }
        counts
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoctorCounts {
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
}

#[cfg(test)]
mod tests;
