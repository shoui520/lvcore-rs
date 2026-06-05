use std::fs;

use super::*;

#[test]
fn loose_movie_resource_resolves_and_reads_movie_file() {
    let dir = tempdir().unwrap();
    let package_root = dir.path().join("_DCT_SAMPLE");
    let movie_root = dir.path().join("_DCT_SAMPLE_MOVIE");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(&movie_root).unwrap();
    fs::write(movie_root.join("12345678"), b"movie bytes").unwrap();

    let package = ReaderBookPackage::new(
        &package_root,
        DetectedPackage {
            root: package_root.clone(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Sample".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );
    let token = ResourceToken::new(&InternalResource::LooseMovie {
        movie_id: "12345678".to_owned(),
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::Video);
    assert_eq!(resource.mime_type.as_deref(), Some("video/mpeg"));
    assert_eq!(resource.byte_len, Some(11));
    assert!(resource.href.is_some());
    assert!(resource.diagnostics.is_empty());
    assert_eq!(package.read_resource(&token).unwrap(), b"movie bytes");
}

#[test]
fn sounddata_resource_resolves_and_reads_wave_record() {
    let dir = tempdir().unwrap();
    let sound_root = dir.path().join("Sound");
    fs::create_dir(&sound_root).unwrap();
    fs::write(
        sound_root.join("SoundData"),
        b"RIFF\x04\x00\x00\x00WAVEignored trailing bytes",
    )
    .unwrap();
    fs::write(
        sound_root.join("WaveFile.map"),
        b"0000000000000000:001b 10\n",
    )
    .unwrap();

    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Sample".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );
    let token = ResourceToken::new(&InternalResource::SoundData { sound_id: 10 }).unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::SoundData);
    assert_eq!(resource.label.as_deref(), Some("SoundData/0000000a"));
    assert_eq!(resource.mime_type.as_deref(), Some("audio/wav"));
    assert!(resource.href.is_some());
    assert!(resource.diagnostics.is_empty());
    assert_eq!(
        package.read_resource(&token).unwrap(),
        b"RIFF\x04\x00\x00\x00WAVE"
    );
}
