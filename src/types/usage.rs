use clewdr_types::QuotaWindowApi;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct QuotaSummary {
    pub session: Option<QuotaWindowApi>,
    pub weekly: Option<QuotaWindowApi>,
    pub model: Option<QuotaWindowApi>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct UsageResponse {
    #[serde(default)]
    five_hour: Option<LegacyWindow>,
    #[serde(default)]
    seven_day: Option<LegacyWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<LegacyWindow>,
    #[serde(default)]
    limits: Vec<UsageLimit>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LegacyWindow {
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct UsageLimit {
    kind: String,
    #[serde(default)]
    percent: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
    #[serde(default)]
    is_active: Option<bool>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    scope: Option<UsageScope>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct UsageScope {
    #[serde(default)]
    model: Option<UsageScopeModel>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct UsageScopeModel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
}

impl UsageLimit {
    fn into_window(self, default_scope: Option<&str>) -> QuotaWindowApi {
        let scope = self
            .scope
            .and_then(|scope| scope.model)
            .and_then(|model| model.display_name.or(model.id))
            .or_else(|| default_scope.map(str::to_string));
        QuotaWindowApi {
            utilization: self.percent,
            resets_at: self.resets_at,
            is_active: self.is_active,
            severity: self.severity,
            scope,
        }
    }
}

fn legacy_window(window: Option<LegacyWindow>, scope: Option<&str>) -> Option<QuotaWindowApi> {
    window.map(|window| QuotaWindowApi {
        utilization: window.utilization,
        resets_at: window.resets_at,
        scope: scope.map(str::to_string),
        ..Default::default()
    })
}

pub fn extract_quota_summary(value: &Value) -> Option<QuotaSummary> {
    let response = serde_json::from_value::<UsageResponse>(value.clone()).ok()?;
    let mut session = None;
    let mut weekly = None;
    let mut model = None;

    for limit in response.limits {
        match limit.kind.as_str() {
            "session" if session.is_none() => session = Some(limit.into_window(None)),
            "weekly_all" if weekly.is_none() => weekly = Some(limit.into_window(None)),
            "weekly_scoped" if model.is_none() => model = Some(limit.into_window(Some("Fable"))),
            _ => {}
        }
    }

    session = session.or_else(|| legacy_window(response.five_hour, None));
    weekly = weekly.or_else(|| legacy_window(response.seven_day, None));
    model = model.or_else(|| legacy_window(response.seven_day_sonnet, Some("Sonnet")));

    (session.is_some() || weekly.is_some() || model.is_some()).then_some(QuotaSummary {
        session,
        weekly,
        model,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::extract_quota_summary;

    #[test]
    fn extracts_three_new_usage_windows() {
        let usage = json!({
            "five_hour": {"utilization": 99.0, "resets_at": "legacy"},
            "limits": [
                {"kind": "session", "percent": 0, "is_active": false, "resets_at": null},
                {"kind": "weekly_all", "percent": 0, "is_active": false, "resets_at": "2026-07-12T15:59:59Z"},
                {
                    "kind": "weekly_scoped",
                    "percent": 1,
                    "is_active": true,
                    "resets_at": "2026-07-12T15:59:59Z",
                    "scope": {"model": {"id": null, "display_name": "Fable"}}
                }
            ]
        });

        let summary = extract_quota_summary(&usage).unwrap();
        assert_eq!(summary.session.unwrap().utilization, Some(0.0));
        assert_eq!(summary.weekly.unwrap().utilization, Some(0.0));
        let model = summary.model.unwrap();
        assert_eq!(model.utilization, Some(1.0));
        assert_eq!(model.scope.as_deref(), Some("Fable"));
    }

    #[test]
    fn falls_back_to_legacy_usage_fields_without_rounding() {
        let usage = json!({
            "five_hour": {"utilization": 2.5, "resets_at": null},
            "seven_day": {"utilization": 7.25, "resets_at": "weekly"},
            "seven_day_sonnet": {"utilization": 12.75, "resets_at": "model"}
        });

        let summary = extract_quota_summary(&usage).unwrap();
        assert_eq!(summary.session.unwrap().utilization, Some(2.5));
        assert_eq!(summary.weekly.unwrap().utilization, Some(7.25));
        assert_eq!(summary.model.unwrap().utilization, Some(12.75));
    }
}
