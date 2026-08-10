/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#[cfg(any(test, ohos_rdb))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColumnKind {
    Blob,
    Text,
}

#[cfg(any(test, ohos_rdb))]
fn read_buffer_as_string(buffer: &[u8]) -> String {
    let len = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..len]).into_owned()
}

#[cfg(any(test, ohos_rdb))]
pub(crate) fn decode_column(column_kind: ColumnKind, size: usize, raw: &[u8]) -> String {
    match column_kind {
        ColumnKind::Blob => String::from_utf8_lossy(&raw[..size]).into_owned(),
        ColumnKind::Text => read_buffer_as_string(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::{ColumnKind, decode_column};

    #[test]
    fn decode_column_blob_preserves_interior_nul() {
        assert_eq!(
            decode_column(ColumnKind::Blob, 3, b"a\0b"),
            String::from("a\0b")
        );
    }

    #[test]
    fn decode_column_blob_ignores_overallocated_buffer() {
        let bytes = b"a\0bXYZ";

        assert_eq!(
            decode_column(ColumnKind::Blob, 3, bytes),
            String::from("a\0b")
        );
    }

    #[test]
    fn decode_column_text_trims_trailing_nul() {
        assert_eq!(
            decode_column(ColumnKind::Text, 4, b"name\0junk"),
            String::from("name")
        );
    }
}
