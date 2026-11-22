use std::{fs::File, io::BufReader, path::Path};

use kamadak_exif::{Error as ExifError, Reader, Tag, Value};

use super::{parse_datetime, ExifInfo, ExifSource};

pub fn read(path: &Path) -> Result<Option<ExifInfo>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let exif = match Reader::new().read_from_container(&mut BufReader::new(file)) {
        Ok(exif) => exif,
        Err(ExifError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };

    let taken_at = [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime]
        .into_iter()
        .find_map(|tag| ascii_value(&exif, tag).and_then(|value| parse_datetime(&value)));
    let make = ascii_value(&exif, Tag::Make);
    let model = ascii_value(&exif, Tag::Model);

    if taken_at.is_none() && make.is_none() && model.is_none() {
        return Ok(None);
    }

    Ok(Some(ExifInfo {
        taken_at,
        source: ExifSource::Native,
        make,
        model,
    }))
}

fn ascii_value(exif: &kamadak_exif::Exif, tag: Tag) -> Option<String> {
    let field = exif.fields().find(|field| field.tag == tag)?;
    let Value::Ascii(values) = &field.value else {
        return None;
    };
    let value = values.first()?;
    let value = String::from_utf8_lossy(value)
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::read;
    use crate::exif::ExifSource;

    #[test]
    fn reads_original_date_and_camera_from_a_tiff_exif_fixture() {
        let file = write_tiff_fixture(true);

        let info = read(file.path()).unwrap().unwrap();

        assert_eq!(
            info.taken_at
                .unwrap()
                .format("%Y:%m:%d %H:%M:%S")
                .to_string(),
            "2023:02:23 12:53:12"
        );
        assert_eq!(info.make.as_deref(), Some("Sony"));
        assert_eq!(info.model.as_deref(), Some("ILCE-7M4"));
        assert!(matches!(info.source, ExifSource::Native));
    }

    #[test]
    fn valid_exif_without_a_date_is_not_a_reader_error() {
        let file = write_tiff_fixture(false);

        let info = read(file.path()).unwrap().unwrap();

        assert_eq!(info.taken_at, None);
        assert_eq!(info.make.as_deref(), Some("Sony"));
    }

    fn write_tiff_fixture(with_dates: bool) -> NamedTempFile {
        let mut bytes = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        push_u16(&mut bytes, 3);
        push_entry(&mut bytes, 0x010f, 2, 5, 50);
        push_entry(&mut bytes, 0x0110, 2, 9, 55);
        push_entry(&mut bytes, 0x8769, 4, 1, 64);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(b"Sony\0");
        bytes.extend_from_slice(b"ILCE-7M4\0");

        assert_eq!(bytes.len(), 64);
        if with_dates {
            push_u16(&mut bytes, 2);
            push_entry(&mut bytes, 0x9003, 2, 20, 94);
            push_entry(&mut bytes, 0x9004, 2, 20, 114);
            push_u32(&mut bytes, 0);
            bytes.extend_from_slice(b"2023:02:23 12:53:12\0");
            bytes.extend_from_slice(b"2020:01:01 00:00:00\0");
        } else {
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, 0);
        }

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();
        file
    }

    fn push_entry(bytes: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
        push_u16(bytes, tag);
        push_u16(bytes, field_type);
        push_u32(bytes, count);
        push_u32(bytes, value);
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
