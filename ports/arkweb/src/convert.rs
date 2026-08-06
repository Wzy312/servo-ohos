//! Pure value/format conversions used by the bridge, kept free of any `servo`/OHOS types so they
//! can be unit-tested on the host. The rest of the crate is `target_env = "ohos"`-only, so this
//! module is compiled only where it is used (ohos) or exercised (host `cfg(test)`).

/// Render an `f64` JS number as the text form ArkWeb's string-typed `runJavaScript` result expects:
/// whole values in `i64` range without a trailing `.0`, `±Infinity` spelled out the JS way. Values
/// at or beyond `2^63` fall through to the full decimal form so the `as i64` cast cannot saturate.
pub(crate) fn format_js_number(number: f64) -> String {
    if number.is_infinite() {
        if number.is_sign_negative() {
            "-Infinity".to_owned()
        } else {
            "Infinity".to_owned()
        }
    } else if number.fract() == 0.0 && number.abs() < 9_223_372_036_854_775_808.0 {
        format!("{}", number as i64)
    } else {
        number.to_string()
    }
}

/// Decode an OHOS key event's unicode value into a printable character, if any. A zero value (no
/// character), an out-of-range value, or a control character yields `None`.
pub(crate) fn printable_char(unicode: i32) -> Option<char> {
    u32::try_from(unicode)
        .ok()
        .filter(|&u| u != 0)
        .and_then(char::from_u32)
        .filter(|c| !c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_js_number_whole_values() {
        assert_eq!(format_js_number(42.0), "42");
        assert_eq!(format_js_number(0.0), "0");
        assert_eq!(format_js_number(-0.0), "0");
        assert_eq!(format_js_number(-7.0), "-7");
        // Large but still inside i64 range: rendered as an integer, not scientific notation.
        assert_eq!(format_js_number(9_000_000_000_000.0), "9000000000000");
    }

    #[test]
    fn format_js_number_fractional_values() {
        assert_eq!(format_js_number(1.5), "1.5");
        assert_eq!(format_js_number(-0.25), "-0.25");
    }

    #[test]
    fn format_js_number_beyond_i64_does_not_saturate() {
        // The old `as i64` fast-path clamped 1e20 to i64::MAX; it must now round-trip instead.
        let big = 1e20;
        let rendered = format_js_number(big);
        assert_ne!(rendered, i64::MAX.to_string());
        assert_eq!(rendered.parse::<f64>().unwrap(), big);
    }

    #[test]
    fn format_js_number_non_finite() {
        assert_eq!(format_js_number(f64::INFINITY), "Infinity");
        assert_eq!(format_js_number(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(format_js_number(f64::NAN), "NaN");
    }

    #[test]
    fn printable_char_maps_text() {
        assert_eq!(printable_char('a' as i32), Some('a'));
        assert_eq!(printable_char('A' as i32), Some('A'));
        assert_eq!(printable_char(0x1F600), Some('😀'));
    }

    #[test]
    fn printable_char_rejects_non_text() {
        assert_eq!(printable_char(0), None); // no character
        assert_eq!(printable_char(-1), None); // out of range
        assert_eq!(printable_char(9), None); // tab (control)
        assert_eq!(printable_char(13), None); // carriage return (control)
        assert_eq!(printable_char(0xD800), None); // lone surrogate, not a scalar value
    }
}
