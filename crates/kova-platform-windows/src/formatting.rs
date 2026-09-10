//! Windows regional formatting, kept separate from translatable UI text.
//! Values passed here are presentation only; comparisons use numeric metadata.
use chrono::{Datelike, Timelike};
use windows::{
    Win32::{Foundation::SYSTEMTIME, Globalization::*},
    core::PCWSTR,
};

pub fn date(date: chrono::DateTime<chrono::Local>, seconds: bool) -> String {
    let time = SYSTEMTIME {
        wYear: date.year() as u16,
        wMonth: date.month() as u16,
        wDay: date.day() as u16,
        wHour: date.hour() as u16,
        wMinute: date.minute() as u16,
        wSecond: date.second() as u16,
        ..Default::default()
    };
    let mut day = [0u16; 128];
    let mut clock = [0u16; 128];
    // Null locale selects the user's Windows regional settings, including overrides.
    let (d, t) = unsafe {
        (
            GetDateFormatEx(
                PCWSTR::null(),
                DATE_SHORTDATE,
                Some(&time),
                PCWSTR::null(),
                Some(&mut day),
                PCWSTR::null(),
            ),
            GetTimeFormatEx(
                PCWSTR::null(),
                if seconds {
                    TIME_FORMAT_FLAGS(0)
                } else {
                    TIME_NOSECONDS
                },
                Some(&time),
                PCWSTR::null(),
                Some(&mut clock),
            ),
        )
    };
    if d > 0 && t > 0 {
        format!(
            "{} {}",
            String::from_utf16_lossy(&day[..d as usize - 1]),
            String::from_utf16_lossy(&clock[..t as usize - 1])
        )
    } else {
        date.format("%Y-%m-%d %H:%M").to_string()
    }
}

pub fn integer(value: u64) -> String {
    let value = value.to_string();
    let input: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let mut output = [0u16; 128];
    let decimal: Vec<u16> = locale(LOCALE_SDECIMAL, ".")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let thousands: Vec<u16> = locale(LOCALE_STHOUSAND, ",")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let format = NUMBERFMTW {
        NumDigits: 0,
        LeadingZero: 1,
        Grouping: 3,
        lpDecimalSep: windows::core::PWSTR(decimal.as_ptr().cast_mut()),
        lpThousandSep: windows::core::PWSTR(thousands.as_ptr().cast_mut()),
        NegativeOrder: 1,
    };
    let n = unsafe {
        GetNumberFormatEx(
            PCWSTR::null(),
            0,
            PCWSTR(input.as_ptr()),
            Some(&format),
            Some(&mut output),
        )
    };
    if n > 0 {
        String::from_utf16_lossy(&output[..n as usize - 1])
    } else {
        value
    }
}

fn locale(kind: u32, fallback: &str) -> String {
    let mut buffer = [0u16; 80];
    let count = unsafe { GetLocaleInfoEx(PCWSTR::null(), kind, Some(&mut buffer)) };
    if count > 0 {
        String::from_utf16_lossy(&buffer[..count as usize - 1])
    } else {
        fallback.into()
    }
}

pub fn bytes(bytes: u64) -> String {
    static DECIMAL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let (divisor, suffix) = if bytes >= 1 << 40 {
        (1u64 << 40, "TiB")
    } else if bytes >= 1 << 30 {
        (1 << 30, "GiB")
    } else if bytes >= 1 << 20 {
        (1 << 20, "MiB")
    } else if bytes >= 1 << 10 {
        (1 << 10, "KiB")
    } else {
        return format!("{bytes} B");
    };
    format!("{:.1} {suffix}", bytes as f64 / divisor as f64)
        .replace('.', DECIMAL.get_or_init(|| locale(LOCALE_SDECIMAL, ".")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn locale_values_are_nonempty_and_binary_units_are_explicit() {
        assert!(!super::date(chrono::Local::now(), true).is_empty());
        assert!(super::bytes(1024).ends_with(" KiB"));
        assert_eq!(super::bytes(13), "13 B");
        assert_eq!(
            super::integer(1_000_000)
                .chars()
                .filter(char::is_ascii_digit)
                .collect::<String>(),
            "1000000"
        );
    }
}
