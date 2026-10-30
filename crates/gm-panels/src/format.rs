//! Date/time and size formatting for panel columns.

use std::time::SystemTime;

fn zoned(t: SystemTime) -> Option<jiff::Zoned> {
    let ts = jiff::Timestamp::try_from(t).ok()?;
    Some(ts.to_zoned(jiff::tz::TimeZone::system()))
}

/// `dd.mm.yy`
pub fn date(t: Option<SystemTime>) -> String {
    t.and_then(zoned).map(|z| z.strftime("%d.%m.%y").to_string()).unwrap_or_default()
}

/// `hh:mm`
pub fn time(t: Option<SystemTime>) -> String {
    t.and_then(zoned).map(|z| z.strftime("%H:%M").to_string()).unwrap_or_default()
}

/// `dd.mm.yy hh:mm` (or with 4-digit year when `width` allows).
pub fn datetime(t: Option<SystemTime>, width: usize) -> String {
    let Some(z) = t.and_then(zoned) else { return String::new() };
    if width >= 19 {
        z.strftime("%d.%m.%Y %H:%M:%S").to_string()
    } else if width >= 16 {
        z.strftime("%d.%m.%Y %H:%M").to_string()
    } else {
        z.strftime("%d.%m.%y %H:%M").to_string()
    }
}

/// Current local time `hh:mm` for the clock.
pub fn clock() -> String {
    jiff::Zoned::now().strftime("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        let t = Some(std::time::SystemTime::now());
        assert_eq!(super::date(t).len(), 8);
        assert_eq!(super::time(t).len(), 5);
        assert_eq!(super::datetime(t, 14).len(), 14);
        assert_eq!(super::date(None), "");
    }
}
