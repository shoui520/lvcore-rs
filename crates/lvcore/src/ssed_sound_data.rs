use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::storage::path_stays_inside_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundDataMapRecord {
    pub sound_id: u32,
    pub raw_sound_id: i64,
    pub offset: u64,
    pub length: u64,
    pub line_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundDataIndex {
    pub directory: PathBuf,
    pub sounddata_path: PathBuf,
    pub map_path: PathBuf,
    pub rows: Vec<SoundDataMapRecord>,
    records: BTreeMap<u32, SoundDataMapRecord>,
}

impl SoundDataIndex {
    pub fn record(&self, sound_id: u32) -> Option<&SoundDataMapRecord> {
        self.records.get(&sound_id)
    }

    pub fn read_record(&self, sound_id: u32) -> Result<Option<Vec<u8>>> {
        let Some(record) = self.record(sound_id) else {
            return Ok(None);
        };
        let file_len = self.sounddata_path.metadata()?.len();
        let Some(end_offset) = record.offset.checked_add(record.length) else {
            return Ok(None);
        };
        if end_offset > file_len {
            return Ok(None);
        }
        let Ok(length) = usize::try_from(record.length) else {
            return Ok(None);
        };
        if !path_stays_inside_root(&self.directory, &self.sounddata_path)? {
            return Err(Error::Driver(format!(
                "SoundData path is outside its loose media root: {}",
                self.sounddata_path.display()
            )));
        }
        let mut file = fs::File::open(&self.sounddata_path)?;
        file.seek(SeekFrom::Start(record.offset))?;
        let mut raw = vec![0_u8; length];
        file.read_exact(&mut raw)?;
        Ok(Some(portable_sounddata_audio_bytes(raw)))
    }
}

pub fn load_sounddata_index(package_root: &Path) -> Result<Option<SoundDataIndex>> {
    let Some(sound_dir) = find_child_casefolded(package_root, "Sound")? else {
        return Ok(None);
    };
    if !sound_dir.is_dir() {
        return Ok(None);
    }
    let Some(sounddata_path) = find_child_casefolded(&sound_dir, "SoundData")? else {
        return Ok(None);
    };
    let Some(map_path) = find_child_casefolded(&sound_dir, "WaveFile.map")? else {
        return Ok(None);
    };
    if !sounddata_path.is_file() || !map_path.is_file() {
        return Ok(None);
    }
    if !path_stays_inside_root(&sound_dir, &sounddata_path)? {
        return Err(Error::Driver(format!(
            "SoundData path is outside its loose media root: {}",
            sounddata_path.display()
        )));
    }
    if !path_stays_inside_root(&sound_dir, &map_path)? {
        return Err(Error::Driver(format!(
            "WaveFile.map path is outside its loose media root: {}",
            map_path.display()
        )));
    }

    let rows = parse_wavefile_map(&fs::read_to_string(&map_path)?);
    if rows.is_empty() {
        return Ok(None);
    }
    let mut records = BTreeMap::new();
    for row in &rows {
        records.entry(row.sound_id).or_insert_with(|| row.clone());
    }
    Ok(Some(SoundDataIndex {
        directory: sound_dir,
        sounddata_path,
        map_path,
        rows,
        records,
    }))
}

pub fn resolve_sounddata_record(
    package_root: &Path,
    sound_id: u32,
) -> Result<Option<SoundDataMapRecord>> {
    Ok(load_sounddata_index(package_root)?.and_then(|index| index.record(sound_id).cloned()))
}

pub fn read_sounddata_record(package_root: &Path, sound_id: u32) -> Result<Option<Vec<u8>>> {
    let Some(index) = load_sounddata_index(package_root)? else {
        return Ok(None);
    };
    index.read_record(sound_id)
}

fn parse_wavefile_map(text: &str) -> Vec<SoundDataMapRecord> {
    let mut rows = Vec::new();
    let mut previous_sound_id: Option<u32> = None;
    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(range), Some(raw_sound_id), None) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Some((offset_hex, length_hex)) = range.split_once(':') else {
            continue;
        };
        let (Ok(offset), Ok(length), Ok(raw_sound_id)) = (
            u64::from_str_radix(offset_hex, 16),
            u64::from_str_radix(length_hex, 16),
            raw_sound_id.parse::<i64>(),
        ) else {
            continue;
        };
        if length == 0 {
            continue;
        }
        let sound_id = if raw_sound_id < 0 {
            previous_sound_id.and_then(|previous| previous.checked_add(1))
        } else {
            u32::try_from(raw_sound_id).ok()
        };
        let Some(sound_id) = sound_id else {
            continue;
        };
        rows.push(SoundDataMapRecord {
            sound_id,
            raw_sound_id,
            offset,
            length,
            line_index: line_index as u32 + 1,
        });
        previous_sound_id = Some(sound_id);
    }
    rows
}

fn portable_sounddata_audio_bytes(mut raw: Vec<u8>) -> Vec<u8> {
    if raw.len() >= 12 && &raw[..4] == b"RIFF" && &raw[8..12] == b"WAVE" {
        let content_size = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
        if let Some(content_size) = content_size.checked_add(8)
            && content_size <= raw.len()
        {
            raw.truncate(content_size);
        }
    }
    raw
}

fn find_child_casefolded(directory: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !directory.is_dir() {
        return Ok(None);
    }
    let wanted = name.to_lowercase();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(found) = path.file_name() else {
            continue;
        };
        if found.to_string_lossy().to_lowercase() == wanted {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavefile_map_recovers_signed_display_bug_rows() {
        let rows = parse_wavefile_map(
            "\
0000000000000000:000c 32767
000000000000000c:000c -3276
",
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].sound_id, 32767);
        assert_eq!(rows[1].sound_id, 32768);
        assert_eq!(rows[1].raw_sound_id, -3276);
    }

    #[test]
    fn sounddata_read_trims_riff_declared_size() {
        let dir = tempfile::tempdir().unwrap();
        let sound_dir = dir.path().join("Sound");
        fs::create_dir(&sound_dir).unwrap();
        let mut sounddata = b"RIFF\x04\x00\x00\x00WAVE".to_vec();
        sounddata.extend_from_slice(b"trailing");
        fs::write(sound_dir.join("SoundData"), sounddata).unwrap();
        fs::write(sound_dir.join("WaveFile.map"), b"0000000000000000:0014 1\n").unwrap();

        let bytes = read_sounddata_record(dir.path(), 1).unwrap().unwrap();

        assert_eq!(bytes, b"RIFF\x04\x00\x00\x00WAVE");
    }

    #[test]
    fn sounddata_read_rejects_map_ranges_outside_sounddata_file() {
        let dir = tempfile::tempdir().unwrap();
        let sound_dir = dir.path().join("Sound");
        fs::create_dir(&sound_dir).unwrap();
        fs::write(sound_dir.join("SoundData"), b"tiny").unwrap();
        fs::write(
            sound_dir.join("WaveFile.map"),
            b"0000000000000001:ffffffffffffffff 1\n",
        )
        .unwrap();

        let bytes = read_sounddata_record(dir.path(), 1).unwrap();

        assert!(bytes.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn sounddata_symlink_escape_is_not_readable() {
        let dir = tempfile::tempdir().unwrap();
        let sound_dir = dir.path().join("Sound");
        fs::create_dir(&sound_dir).unwrap();
        let outside = dir.path().join("outside.wav");
        fs::write(&outside, b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, sound_dir.join("SoundData")).unwrap();
        fs::write(sound_dir.join("WaveFile.map"), b"0000000000000000:0007 1\n").unwrap();

        let error = read_sounddata_record(dir.path(), 1).unwrap_err();
        assert!(error.to_string().contains("outside its loose media root"));
    }
}
