use super::*;
use std::io::Write;
use std::time::Duration;
#[test]
fn versions_are_strict_and_ordered() {
    let base = format!("v{}.{}.{}", 1, 2, 3);
    let stable = Version::parse(&base).unwrap();
    let rc2 = format!("{}-{}{}", base, "rc", ".2");
    let rc1 = format!("{}-{}{}", base, "rc", ".1");
    assert!(Version::parse(&rc2).unwrap().cmp(&stable).is_lt());
    assert!(Version::parse(&rc2)
        .unwrap()
        .cmp(&Version::parse(&rc1).unwrap())
        .is_gt());
    let alpha = Version::parse(&format!("{}-alpha.1", base)).unwrap();
    let beta = Version::parse(&format!("{}-beta.1", base)).unwrap();
    assert!(alpha.cmp(&beta).is_lt());
    assert!(beta.cmp(&Version::parse(&rc1).unwrap()).is_lt());
    assert!(Version::parse(&rc1).unwrap().cmp(&stable).is_lt());
    let short = format!("v{}.{}", 1, 2);
    assert!(Version::parse(&short).is_none());
    let dev = format!("{}-{}{}", base, "dev", ".1");
    assert!(Version::parse(&dev).is_none());
    for value in ["+1", " 1", "01", "1 ", "0"] {
        let tag = format!("v{}.{}.{}-rc.{value}", 1, 2, 3);
        assert!(Version::parse(&tag).is_none());
    }
    let base = format!("v{}.{}.{}", 1, 2, 3);
    assert!(Version::parse(&format!("{base}:")).is_none());
    assert!(Version::parse(&format!("{base}-rc.1:tail")).is_none());
}

#[test]
fn exact_malformed_core_and_build_metadata_are_rejected_with_valid_neighbors() {
    let core = |patch: &str| format!("v{}.{}.{}", 0, 16, patch);
    assert!(Version::parse(&core("00")).is_none());
    assert!(Version::parse(&core("0")).is_some());
    assert!(Version::parse(&format!("v{}.{}.{}", 0, 17, 0)).is_some());
    let build = format!("v{}.{}.{}+{}.{}", 0, 19, 0, "build", 7);
    assert!(Version::parse(&build).is_none());
    assert!(Version::parse(&format!("v{}.{}.{}", 0, 18, 0)).is_some());
    assert!(Version::parse(&format!("v{}.{}.{}", 0, 19, 0)).is_some());
}

#[test]
fn release_selection_filters_channel_drafts_and_downgrades() {
    let current = Version::parse(&format!("v{}.{}.{}-rc.{}", 1, 2, 3, 1)).unwrap();
    let tags = [
        (format!("v{}.{}.{}-alpha.{}", 1, 2, 3, 2), false, true),
        (format!("v{}.{}.{}-beta.{}", 1, 2, 3, 2), false, true),
        (format!("v{}.{}.{}-rc.{}", 1, 2, 3, 2), false, true),
        (format!("v{}.{}.{}", 1, 2, 4), false, false),
        (format!("v{}.{}.{}", 9, 0, 0), true, false),
        (format!("v{}.{}.{}", 1, 2, 2), false, false),
        (format!("v{}.{}.{}-rc.{}", 1, 2, 3, 3), false, false),
        ("garbage".to_owned(), false, false),
    ];
    let body = serde_json::to_string(
        &tags
            .iter()
            .map(|(tag, draft, prerelease)| {
                serde_json::json!({"tag_name": tag, "draft": draft, "prerelease": prerelease})
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        parse_releases(&body, &current).unwrap().unwrap().tag,
        format!("v{}.{}.{}", 1, 2, 4)
    );
    assert!(parse_releases("{}", &current).is_err());
    assert!(parse_releases(
        "[]",
        &Version::parse(&format!("v{}.{}.{}", 1, 2, 4)).unwrap()
    )
    .unwrap()
    .is_none());
}

#[test]
fn stable_channel_rejects_prereleases() {
    let current = Version::parse(&format!("v{}.{}.{}", 1, 2, 3)).unwrap();
    let prerelease = format!("v{}.{}.{}-rc.{}", 1, 2, 5, 1);
    let stable = format!("v{}.{}.{}", 1, 2, 4);
    let body = serde_json::json!([
        {"tag_name": prerelease, "draft": false, "prerelease": true},
        {"tag_name": stable, "draft": false, "prerelease": false}
    ])
    .to_string();
    assert_eq!(
        parse_releases(&body, &current).unwrap().unwrap().tag,
        stable
    );
}

#[test]
fn cached_latest_preserves_installed_channel() {
    let stable_tag = format!("v{}.{}.{}", 1, 2, 3);
    let prerelease_tag = format!("{}-rc.{}", stable_tag, 1);
    let stable = Version::parse(&stable_tag).unwrap();
    let prerelease = Version::parse(&prerelease_tag).unwrap();
    let stable_newer = format!("v{}.{}.{}", 1, 2, 4);
    let prerelease_newer = format!("{}-rc.{}", stable_newer, 1);
    let stable_cache = serde_json::json!({
        "channel": "stable",
        "latest": prerelease_newer
    });
    assert!(cached_latest(&stable_cache, &stable).is_none());
    let malformed_cache = serde_json::json!({
        "channel": "stable",
        "latest": "not-a-version"
    });
    assert!(cached_latest(&malformed_cache, &stable).is_none());
    let all_cache = serde_json::json!({
        "channel": "all",
        "latest": stable_newer
    });
    assert_eq!(
        cached_latest(&all_cache, &prerelease).unwrap().tag,
        stable_newer
    );
    let prerelease_cache = serde_json::json!({
        "channel": "all",
        "latest": prerelease_newer
    });
    assert!(cached_latest(&prerelease_cache, &prerelease).is_some());
    assert!(!cache_is_fresh(now() + 1, now()));
}

#[test]
fn release_body_limit_is_bounded_beyond_old_cap() {
    let path = temp_file("body-limit").unwrap();
    let mut file = File::create(&path).unwrap();
    file.write_all(&vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(read_release_body(&path).is_ok());
    file.write_all(&vec![b' '; MAX_RELEASE_BYTES as usize])
        .unwrap();
    drop(file);
    assert!(read_release_body(&path).is_err());
    let _ = fs::remove_file(path);
}

#[cfg(unix)]
#[test]
fn cache_rejects_symlink_and_keeps_private_modes() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = crate::test_root::temp("cache-test");
    let cache_dir = root.join("soulmate");
    let cache = cache_dir.join("update.json");
    write_cache(&cache, None, "all", None).unwrap();
    assert_eq!(
        fs::metadata(&cache_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let target = root.join("target");
    fs::write(&target, b"x").unwrap();
    fs::remove_file(&cache).unwrap();
    symlink(&target, &cache).unwrap();
    assert!(write_cache(&cache, None, "all", None).is_err());
    let _ = fs::remove_file(&cache);
    let _ = fs::remove_file(&target);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cache_recovers_an_interrupted_temporary_write() {
    let root = crate::test_root::temp("cache-temp-test");
    let cache = root.join("soulmate/update.json");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let stale = cache.with_file_name("update.json.tmp-interrupted");
    fs::write(&stale, b"partial").unwrap();
    let latest = format!("v{}.{}.{}-rc.{}", 0, 14, 0, 2);
    write_cache(&cache, Some(&latest), "all", None).unwrap();
    assert!(!stale.exists());
    assert!(regular(&cache));
    assert!(fs::read_dir(cache.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .all(|entry| entry.file_name() != "update.json.tmp-interrupted"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn root_home_and_cache_directories_are_rejected() {
    assert!(cache_root_from(None, Some(PathBuf::from("/"))).is_none());
    assert!(cache_root_from(Some(PathBuf::from("/")), Some(PathBuf::from("/tmp/home"))).is_none());
    assert!(cache_root_from(Some(PathBuf::from("/tmp/cache")), None).is_some());
}

#[cfg(unix)]
#[test]
fn marker_lock_has_contention_and_stale_recovery() {
    let root = crate::test_root::temp("lock-test");
    let marker = root.join("soulmate/update.lock");
    let first = acquire_marker(&marker).unwrap();
    assert!(acquire_marker(&marker).is_none());
    drop(first);
    let old = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&marker)
        .unwrap();
    old.set_modified(SystemTime::now() - Duration::from_secs(601))
        .unwrap();
    drop(old);
    assert!(acquire_marker(&marker).is_some());
    let _ = fs::remove_file(&marker);
    let _ = fs::remove_dir_all(root);
}
