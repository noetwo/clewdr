pub fn mask_str(s: &str, visible: usize) -> String {
    if s.len() > visible * 2 {
        format!("{}...{}", &s[..visible], &s[s.len() - visible..])
    } else {
        s.to_string()
    }
}

pub fn format_timestamp(ts: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64((ts * 1000) as f64));
    to_locale_string(&date)
}

pub fn format_iso_beijing(iso: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(iso));
    if date.get_time().is_nan() {
        return "N/A".into();
    }
    let beijing = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(
        date.get_time() + 8.0 * 60.0 * 60.0 * 1000.0,
    ));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        beijing.get_utc_full_year(),
        beijing.get_utc_month() + 1,
        beijing.get_utc_date(),
        beijing.get_utc_hours(),
        beijing.get_utc_minutes(),
        beijing.get_utc_seconds(),
    )
}

fn to_locale_string(date: &js_sys::Date) -> String {
    date.to_locale_string("default", &wasm_bindgen::JsValue::UNDEFINED)
        .as_string()
        .unwrap_or_else(|| "N/A".into())
}

pub fn copy_to_clipboard(text: String) {
    wasm_bindgen_futures::spawn_local(async move {
        let window = web_sys::window().unwrap();
        let clipboard = window.navigator().clipboard();
        let _ = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&text)).await;
    });
}
