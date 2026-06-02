use super::*;

#[test]
fn parses_lved_addr_links() {
    let parsed = parse_lved_address(r#"href="lved.addr0000f768:00a2""#).unwrap();
    assert_eq!(parsed.block, 0x0000f768);
    assert_eq!(parsed.offset, 0x00a2);
}

#[test]
fn britannica_whatday_table_drops_spacer_column() {
    let html = r#"<BODY><TABLE><TR><TD colSpan=3>head</TD></TR><TR><TD>603年</TD><TD>　</TD><TD>event</TD></TR></TABLE></BODY>"#;
    let rendered = render_britannica_html_fragment(html);
    assert!(rendered.contains("colSpan=2"));
    assert!(rendered.contains("<TD>603年</TD><TD>event</TD>"));
    assert!(!rendered.contains("<BODY>"));
}

#[cfg(unix)]
#[test]
fn pcmu_record_symlink_escape_is_not_readable() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dict");
    let pcmu = package.join("_PCM_U");
    std::fs::create_dir_all(&pcmu).unwrap();
    std::fs::write(pcmu.join("WaveFile.map"), b"sound.bin 123\n").unwrap();
    let outside = dir.path().join("outside.bin");
    std::fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, pcmu.join("sound.bin")).unwrap();

    let error = read_pcmu_record(&package, 123).unwrap_err();
    assert!(error.to_string().contains("outside its loose media root"));
}

#[cfg(unix)]
#[test]
fn britannica_whatday_symlink_escape_is_not_readable() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dict");
    let whatday = package.join("Media").join("whatday");
    std::fs::create_dir_all(&whatday).unwrap();
    let outside = dir.path().join("outside.body");
    std::fs::write(&outside, b"<body>outside</body>").unwrap();
    std::os::unix::fs::symlink(&outside, whatday.join("1-1.body")).unwrap();

    let error = parse_britannica_whatday_file(&package, "Media", "whatday/1-1.body").unwrap_err();
    assert!(error.to_string().contains("outside its media root"));
    assert!(!has_britannica_whatday_files(&package).unwrap());
    assert!(
        discover_britannica_whatday_paths(&package)
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn britannica_top_dat_symlink_escape_is_not_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dict");
    let top = package.join("Media").join("top");
    std::fs::create_dir_all(&top).unwrap();
    let outside = dir.path().join("top_people.dat");
    std::fs::write(&outside, b"id\ntitle\ndesc\n00000001:0000\nimage.jpg\n").unwrap();
    std::os::unix::fs::symlink(&outside, top.join("top_people.dat")).unwrap();

    assert!(!has_britannica_top_dat_files(&package).unwrap());
    assert!(
        discover_britannica_top_dat_files(&package)
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn loose_movie_symlink_escape_is_not_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dict");
    let movie = package.join("_MOVIE");
    std::fs::create_dir_all(&movie).unwrap();
    let outside = dir.path().join("00000001");
    std::fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, movie.join("00000001")).unwrap();

    let error = find_movie_file(&package, "00000001").unwrap_err();
    assert!(error.to_string().contains("outside its loose media root"));
}

#[cfg(unix)]
#[test]
fn loose_media_root_symlink_escape_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dict");
    std::fs::create_dir_all(&package).unwrap();
    let outside = dir.path().join("outside-media");
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(outside.join("whatday")).unwrap();
    std::fs::write(outside.join("whatday").join("1-1.body"), b"<body>x</body>").unwrap();
    std::os::unix::fs::symlink(&outside, package.join("Media")).unwrap();

    let roots = discover_britannica_media_roots(&package).unwrap();
    assert!(roots.is_empty());
}
