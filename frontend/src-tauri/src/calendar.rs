use std::time::Duration;

use tauri::{AppHandle, Emitter, Runtime};
use url::Url;

const MAX_CALENDAR_BYTES: u64 = 10 * 1024 * 1024;
const CALENDAR_TICK_SECONDS: u64 = 5;

fn is_allowed_google_calendar_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    if host != "calendar.google.com" && host != "calendar.googleusercontent.com" {
        return false;
    }

    let path = url.path().to_ascii_lowercase();
    path.contains("/calendar/ical/") && (path.ends_with("/basic.ics") || path.ends_with("/full.ics"))
}

fn parse_google_calendar_url(raw_url: &str) -> Result<Url, String> {
    let url = Url::parse(raw_url.trim()).map_err(|_| {
        "Enter the Secret address in iCal format from Google Calendar settings.".to_string()
    })?;

    if !is_allowed_google_calendar_url(&url) {
        return Err(
            "Only a Google Calendar HTTPS iCal URL ending in basic.ics or full.ics is allowed."
                .to_string(),
        );
    }

    Ok(url)
}

#[tauri::command]
pub async fn fetch_google_calendar_ics(ical_url: String) -> Result<String, String> {
    let url = parse_google_calendar_url(&ical_url)?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if is_allowed_google_calendar_url(attempt.url()) && attempt.previous().len() < 3 {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| format!("Could not initialize calendar connection: {error}"))?;

    let response = client
        .get(url)
        .send()
        .await
        // A private iCal URL is a credential. Do not include reqwest's error
        // display here because it can contain the full requested URL.
        .map_err(|_| {
            "Could not download Google Calendar. Check the network connection and calendar address."
                .to_string()
        })?;

    if !response.status().is_success() {
        return Err(format!(
            "Google Calendar returned HTTP status {}.",
            response.status().as_u16()
        ));
    }

    if response.content_length().is_some_and(|size| size > MAX_CALENDAR_BYTES) {
        return Err("Google Calendar feed is larger than the 10 MB safety limit.".to_string());
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Could not read Google Calendar response: {error}"))?;

    if bytes.len() as u64 > MAX_CALENDAR_BYTES {
        return Err("Google Calendar feed is larger than the 10 MB safety limit.".to_string());
    }

    String::from_utf8(bytes.to_vec())
        .map_err(|_| "Google Calendar returned invalid text data.".to_string())
}

pub fn start_calendar_tick_emitter<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(CALENDAR_TICK_SECONDS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            if app.emit("calendar-automation-tick", ()).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_google_private_ical_urls() {
        let url = Url::parse(
            "https://calendar.google.com/calendar/ical/user%40example.com/private-token/basic.ics",
        )
        .unwrap();

        assert!(is_allowed_google_calendar_url(&url));
    }

    #[test]
    fn rejects_non_google_and_non_https_urls() {
        let attacker = Url::parse("https://example.com/calendar/ical/a/basic.ics").unwrap();
        let insecure = Url::parse(
            "http://calendar.google.com/calendar/ical/user%40example.com/private/basic.ics",
        )
        .unwrap();

        assert!(!is_allowed_google_calendar_url(&attacker));
        assert!(!is_allowed_google_calendar_url(&insecure));
    }
}
