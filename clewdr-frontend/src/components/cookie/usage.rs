use clewdr_types::QuotaWindowApi;
use leptos::prelude::*;

use crate::{i18n::use_i18n, types::CookieStatus, utils::format_iso_beijing};

fn format_percent(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}%")
    } else {
        format!("{value:.1}%")
    }
}

fn actual_usage(window: &QuotaWindowApi, is_model: bool) -> String {
    let Some(utilization) = window.utilization else {
        return "N/A".to_string();
    };
    let i = use_i18n();
    let percent = format_percent(utilization);
    if window.is_active == Some(false) {
        return i.tf("cookieStatus.quota.inactive", &[("percent", &percent)]);
    }
    if is_model {
        let remaining = format_percent((100.0 - utilization).clamp(0.0, 100.0));
        return i.tf(
            "cookieStatus.quota.remaining",
            &[("percent", &percent), ("remaining", &remaining)],
        );
    }
    percent
}

#[component]
pub fn UsageDetails(cookie: CookieStatus) -> impl IntoView {
    let i = use_i18n();
    let rows = [
        (
            i.t("cookieStatus.quota.session"),
            cookie.session_quota,
            false,
        ),
        (
            i.t("cookieStatus.quota.sevenDay"),
            cookie.weekly_quota,
            false,
        ),
        (
            i.t("cookieStatus.quota.modelScoped"),
            cookie.model_quota,
            true,
        ),
    ]
    .into_iter()
    .filter_map(|(label, window, is_model)| {
        window.map(|window| {
            let usage = actual_usage(&window, is_model);
            let reset = window
                .resets_at
                .as_deref()
                .map(format_iso_beijing)
                .unwrap_or_else(|| i.t("cookieStatus.quota.noReset"));
            (label, usage, reset)
        })
    })
    .collect::<Vec<_>>();

    if rows.is_empty() {
        return None;
    }

    Some(view! {
        <div class="quota-table-wrap">
            <table class="quota-table">
                <thead>
                    <tr>
                        <th>{i.t("cookieStatus.quota.windowHeader")}</th>
                        <th>{i.t("cookieStatus.quota.usageHeader")}</th>
                        <th>{i.t("cookieStatus.quota.resetHeader")}</th>
                    </tr>
                </thead>
                <tbody>
                    {rows.into_iter().map(|(label, usage, reset)| view! {
                        <tr>
                            <td>{label}</td>
                            <td>{usage}</td>
                            <td>{reset}</td>
                        </tr>
                    }).collect::<Vec<_>>()}
                </tbody>
            </table>
        </div>
    })
}
