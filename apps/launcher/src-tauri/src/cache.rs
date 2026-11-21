//! The download cache: the release files Quiver downloaded, kept to install again without downloading them, and as a
//! backup of what was published. Opt-in (the page's Settings › Cache), in a folder of the player's choosing.
//!
//! Each file is kept by its SHA-256, as `<cache>/<hex>/<filename>`: one copy for every app and version that has it,
//! checked against its name whenever it's used, and addressed the way a content network (IPFS) would. `index.json`
//! beside them says whose each is, how large, and when it was last kept or used, which rotation goes by.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const INDEX: &str = "index.json";

/// One writer at a time on a cache's index: installs and the page's fetches can finish together.
static LOCK: Mutex<()> = Mutex::new(());

/// What an install or a fetch asks of the cache.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CacheUse {
    /// The cache folder.
    pub dir: String,
    /// Keep the downloaded file in it; else only take a file from it.
    #[serde(default)]
    pub keep: bool,
    /// The app's id, the version and the system the file is for: whose it is, in the index.
    pub app: String,
    pub version: String,
    #[serde(default)]
    pub os: Option<String>,
    /// Installed only: the app's files of other versions go once this one is in, so the cache has its latest installed
    /// version alone (with every system's files of it, when those are kept).
    #[serde(default)]
    pub only_this: bool,
    /// The most the cache keeps, in bytes; none for no limit.
    #[serde(default)]
    pub limit: Option<u64>,
    /// Files never rotated out (`<hex>`): those of the versions installed now.
    #[serde(default)]
    pub protect: Vec<String>,
}

/// A file in the cache, as the index has it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cached {
    /// Its SHA-256, in hex: its folder's name.
    pub hash: String,
    pub filename: String,
    pub size: u64,
    /// The apps and versions it's a file of.
    #[serde(default)]
    pub apps: Vec<Owner>,
    /// Milliseconds since 1970: when it was kept, and when last installed from.
    pub cached_at: u64,
    pub used_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub app: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
}

/// How full the cache is, for its settings page.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub files: usize,
    pub bytes: u64,
    pub entries: Vec<Cached>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The hex of a `sha256:<hex>` checksum (or of a bare hex), lower case; none for anything else.
pub fn hex_of(checksum: &str) -> Option<String> {
    let hex = checksum.trim().strip_prefix("sha256:").unwrap_or(checksum.trim()).to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

fn read_index(dir: &Path) -> Vec<Cached> {
    fs::read(dir.join(INDEX)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Written whole beside the old and moved over it, so a crash never leaves half an index.
fn write_index(dir: &Path, entries: &[Cached]) -> io::Result<()> {
    let tmp = dir.join(format!("{INDEX}.tmp"));
    fs::write(&tmp, serde_json::to_vec_pretty(entries).map_err(io::Error::other)?)?;
    fs::rename(tmp, dir.join(INDEX))
}

fn path_of(dir: &Path, entry: &Cached) -> PathBuf {
    dir.join(&entry.hash).join(&entry.filename)
}

/// The SHA-256 of a file, in hex.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// A copy of the cached file with this checksum at `to`, checked against it, for an install to use in place of
/// downloading it. None when it isn't cached, or what's there isn't that file any more (then it's dropped).
pub fn take(cache: &CacheUse, checksum: &str, to: &Path) -> Option<()> {
    let hex = hex_of(checksum)?;
    let dir = Path::new(&cache.dir);
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut entries = read_index(dir);
    let at = entries.iter().position(|e| e.hash == hex)?;
    let from = path_of(dir, &entries[at]);
    if hash_file(&from).ok().as_deref() != Some(hex.as_str()) {
        // Changed or gone under it: never installed, and no longer listed.
        let _ = fs::remove_dir_all(dir.join(&hex));
        entries.remove(at);
        let _ = write_index(dir, &entries);
        return None;
    }
    fs::copy(&from, to).ok()?;
    entries[at].used_at = now();
    let _ = write_index(dir, &entries);
    Some(())
}

/// Whether these files (`<hex>` or `sha256:<hex>`) are in the cache, by the index: the page's fetches skip those.
pub fn has(dir: &Path, checksums: &[String]) -> Vec<bool> {
    let held: HashSet<String> = read_index(dir).into_iter().map(|e| e.hash).collect();
    checksums.iter().map(|c| hex_of(c).is_some_and(|h| held.contains(&h))).collect()
}

/// Keeps a downloaded file (already checked to be `hash`), as `filename`, for the app and version `cache` names; then
/// rotates the cache down to its limit. A hard link where the cache is on the same drive, else a copy.
pub fn keep(cache: &CacheUse, file: &Path, filename: &str, hash: &str) -> io::Result<()> {
    let hex = hex_of(hash).ok_or_else(|| io::Error::other("not a SHA-256"))?;
    let filename = crate::install::plain_name(filename).map_err(io::Error::other)?.to_string();
    // A host can name a file without its extension ("Game-v1.2-Windows"): kept named for what's in it, as an install
    // names it, so it opens as what it is.
    let filename = crate::install::named_for_contents(&filename, file).unwrap_or(filename);
    let dir = Path::new(&cache.dir);
    fs::create_dir_all(dir)?;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut entries = read_index(dir);
    let owner = Owner { app: cache.app.clone(), version: cache.version.clone(), os: cache.os.clone() };
    let at = now();
    match entries.iter_mut().find(|e| e.hash == hex) {
        Some(entry) if path_of(dir, entry).is_file() => {
            named_for_contents(dir, entry);
            if !entry.apps.contains(&owner) {
                entry.apps.push(owner);
            }
            entry.used_at = at;
        }
        _ => {
            entries.retain(|e| e.hash != hex);
            let folder = dir.join(&hex);
            fs::create_dir_all(&folder)?;
            let to = folder.join(&filename);
            let part = folder.join(format!("{filename}.part"));
            if fs::hard_link(file, &part).is_err() {
                fs::copy(file, &part)?;
            }
            fs::rename(&part, &to)?;
            let size = fs::metadata(&to)?.len();
            entries.push(Cached { hash: hex.clone(), filename, size, apps: vec![owner], cached_at: at, used_at: at });
        }
    }
    // Installed only: this app's latest installed version alone, its others gone (or left to the apps that share them).
    if cache.only_this {
        for entry in entries.iter_mut().filter(|e| e.hash != hex) {
            entry.apps.retain(|o| o.app != cache.app || o.version == cache.version);
        }
        let orphans: Vec<String> = entries.iter().filter(|e| e.apps.is_empty()).map(|e| e.hash.clone()).collect();
        for hash in &orphans {
            let _ = fs::remove_dir_all(dir.join(hash));
        }
        entries.retain(|e| !e.apps.is_empty());
    }
    let mut protect: HashSet<String> = cache.protect.iter().filter_map(|c| hex_of(c)).collect();
    protect.insert(hex);
    rotate(dir, &mut entries, cache.limit, &protect);
    write_index(dir, &entries)
}

/// Removes the least recently used files until the cache is within `limit`, never one in `protect`.
fn rotate(dir: &Path, entries: &mut Vec<Cached>, limit: Option<u64>, protect: &HashSet<String>) {
    let Some(limit) = limit else { return };
    let mut total: u64 = entries.iter().map(|e| e.size).sum();
    if total <= limit {
        return;
    }
    let mut order: Vec<usize> = (0..entries.len()).filter(|&i| !protect.contains(&entries[i].hash)).collect();
    order.sort_by_key(|&i| entries[i].used_at);
    let mut gone = HashSet::new();
    for i in order {
        if total <= limit {
            break;
        }
        let _ = fs::remove_dir_all(dir.join(&entries[i].hash));
        total -= entries[i].size;
        gone.insert(entries[i].hash.clone());
    }
    entries.retain(|e| !gone.contains(&e.hash));
}

/// Rotates the cache down to `limit` now, as when the player lowers it.
pub fn trim(dir: &Path, limit: Option<u64>, protect: &[String]) -> io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut entries = read_index(dir);
    let protect: HashSet<String> = protect.iter().filter_map(|c| hex_of(c)).collect();
    rotate(dir, &mut entries, limit, &protect);
    if dir.exists() {
        write_index(dir, &entries)?;
    }
    Ok(())
}

/// A kept file with no extension named for what's in it ("Game-Windows" a zip becomes "Game-Windows.zip"), as one
/// kept before the cache did so was not. Whether it changed.
fn named_for_contents(dir: &Path, entry: &mut Cached) -> bool {
    let from = path_of(dir, entry);
    let Some(named) = crate::install::named_for_contents(&entry.filename, &from) else { return false };
    if fs::rename(&from, dir.join(&entry.hash).join(&named)).is_err() {
        return false;
    }
    entry.filename = named;
    true
}

/// What's in the cache, with files gone from it by hand left out of the index, and any without an extension named
/// for what's in them.
pub fn status(dir: &Path) -> Status {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut entries = read_index(dir);
    let before = entries.len();
    entries.retain(|e| path_of(dir, e).is_file());
    let renamed = entries.iter_mut().fold(false, |any, e| named_for_contents(dir, e) || any);
    if entries.len() != before || renamed {
        let _ = write_index(dir, &entries);
    }
    Status { files: entries.len(), bytes: entries.iter().map(|e| e.size).sum(), entries }
}

/// Removes these files (`<hex>`), or every one Quiver kept when `hashes` is none. Only what the index names goes, so a
/// cache folder the player pointed at a folder of their own never loses their files.
pub fn remove(dir: &Path, hashes: Option<&[String]>) -> io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut entries = read_index(dir);
    let wanted: Option<HashSet<String>> = hashes.map(|h| h.iter().filter_map(|c| hex_of(c)).collect());
    entries.retain(|e| {
        let goes = wanted.as_ref().is_none_or(|w| w.contains(&e.hash));
        if goes {
            let _ = fs::remove_dir_all(dir.join(&e.hash));
        }
        !goes
    });
    if dir.exists() {
        write_index(dir, &entries)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        (tmp, cache)
    }

    fn uses(cache: &Path, app: &str, version: &str) -> CacheUse {
        CacheUse { dir: cache.to_string_lossy().into(), keep: true, app: app.into(), version: version.into(), ..Default::default() }
    }

    fn put(tmp: &Path, cache: &CacheUse, name: &str, bytes: &[u8]) -> String {
        let file = tmp.join(format!("dl-{name}"));
        fs::write(&file, bytes).unwrap();
        let hash = sha(bytes);
        keep(cache, &file, name, &hash).unwrap();
        hash
    }

    #[test]
    fn keeps_a_file_by_its_hash_and_gives_it_back_checked() {
        let (tmp, cache) = setup();
        let hash = put(tmp.path(), &uses(&cache, "a", "1.0"), "game.zip", b"game files");
        assert_eq!(fs::read(cache.join(&hash).join("game.zip")).unwrap(), b"game files");
        assert_eq!(has(&cache, &[format!("sha256:{hash}"), sha(b"other")]), vec![true, false]);
        let to = tmp.path().join("again.zip");
        assert!(take(&uses(&cache, "a", "1.0"), &format!("sha256:{hash}"), &to).is_some());
        assert_eq!(fs::read(&to).unwrap(), b"game files");
        let status = status(&cache);
        assert_eq!((status.files, status.bytes), (1, 10));
        assert_eq!(status.entries[0].apps, vec![Owner { app: "a".into(), version: "1.0".into(), os: None }]);
    }

    #[test]
    fn never_gives_back_a_file_that_changed_and_forgets_it() {
        let (tmp, cache) = setup();
        let hash = put(tmp.path(), &uses(&cache, "a", "1.0"), "game.zip", b"game files");
        fs::write(cache.join(&hash).join("game.zip"), b"tampered").unwrap();
        assert!(take(&uses(&cache, "a", "1.0"), &hash, &tmp.path().join("x")).is_none());
        assert!(!tmp.path().join("x").exists());
        assert_eq!(status(&cache).files, 0);
        assert!(take(&uses(&cache, "a", "1.0"), "not a checksum", &tmp.path().join("x")).is_none());
    }

    #[test]
    fn one_copy_for_every_app_that_has_the_file() {
        let (tmp, cache) = setup();
        put(tmp.path(), &uses(&cache, "a", "1.0"), "game.zip", b"same");
        put(tmp.path(), &uses(&cache, "b", "2.0"), "other-name.zip", b"same");
        let status = status(&cache);
        assert_eq!(status.files, 1);
        assert_eq!(status.entries[0].apps.iter().map(|o| o.app.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn installed_only_keeps_an_apps_latest_file_alone() {
        let (tmp, cache) = setup();
        let only = |v: &str| CacheUse { only_this: true, ..uses(&cache, "a", v) };
        let old = put(tmp.path(), &only("1.0"), "v1.zip", b"one");
        let shared = put(tmp.path(), &uses(&cache, "b", "1.0"), "b.zip", b"shared");
        put(tmp.path(), &uses(&cache, "a", "1.0"), "b.zip", b"shared");
        let new = put(tmp.path(), &only("2.0"), "v2.zip", b"two");
        let left: HashSet<String> = status(&cache).entries.into_iter().map(|e| e.hash).collect();
        assert!(!left.contains(&old) && left.contains(&new));
        // Another app's file stays, no longer counted as this one's.
        assert!(left.contains(&shared));
        let b = status(&cache).entries.into_iter().find(|e| e.hash == shared).unwrap();
        assert_eq!(b.apps.iter().map(|o| o.app.as_str()).collect::<Vec<_>>(), vec!["b"]);
    }

    #[test]
    fn installed_only_keeps_every_systems_file_of_the_version_installed() {
        let (tmp, cache) = setup();
        let only = |v: &str| CacheUse { only_this: true, ..uses(&cache, "a", v) };
        let old = put(tmp.path(), &only("1.0"), "v1.zip", b"one");
        let win = put(tmp.path(), &only("2.0"), "v2-windows.zip", b"two for windows");
        let linux = put(tmp.path(), &only("2.0"), "v2-linux.tar.gz", b"two for linux");
        let left: HashSet<String> = status(&cache).entries.into_iter().map(|e| e.hash).collect();
        assert_eq!(left, HashSet::from([win, linux]));
        assert!(!cache.join(old).exists());
    }

    #[test]
    fn rotates_the_least_recently_used_out_but_never_a_protected_one() {
        let (tmp, cache) = setup();
        let first = put(tmp.path(), &uses(&cache, "a", "1"), "a", &[1u8; 40]);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = put(tmp.path(), &uses(&cache, "b", "1"), "b", &[2u8; 40]);
        std::thread::sleep(std::time::Duration::from_millis(5));
        // Within 100 bytes, with the first protected (installed): the second goes to make room.
        let limited = CacheUse { limit: Some(100), protect: vec![first.clone()], ..uses(&cache, "c", "1") };
        let third = put(tmp.path(), &limited, "c", &[3u8; 40]);
        let left: Vec<String> = status(&cache).entries.into_iter().map(|e| e.hash).collect();
        assert!(left.contains(&first) && left.contains(&third) && !left.contains(&second));
        assert!(!cache.join(&second).exists());
        // Lowered below what's protected: those stay all the same.
        trim(&cache, Some(10), &[first.clone(), third.clone()]).unwrap();
        assert_eq!(status(&cache).files, 2);
        trim(&cache, Some(50), &[first.clone()]).unwrap();
        assert_eq!(status(&cache).entries.into_iter().map(|e| e.hash).collect::<Vec<_>>(), vec![first]);
    }

    #[test]
    fn removing_takes_only_what_quiver_kept() {
        let (tmp, cache) = setup();
        let a = put(tmp.path(), &uses(&cache, "a", "1"), "a.zip", b"a");
        put(tmp.path(), &uses(&cache, "b", "1"), "b.zip", b"b");
        fs::write(cache.join("players-own.txt"), b"mine").unwrap();
        remove(&cache, Some(&[a])).unwrap();
        assert_eq!(status(&cache).files, 1);
        remove(&cache, None).unwrap();
        assert_eq!(status(&cache).files, 0);
        assert!(cache.join("players-own.txt").exists());
    }

    #[test]
    fn a_file_without_an_extension_is_kept_named_for_what_it_is() {
        let (tmp, cache) = setup();
        let zip = b"PK\x03\x04 the rest of a zip";
        let hash = put(tmp.path(), &uses(&cache, "mk64", "0.9.2"), "MarioKart64Recompiled-v0.9.2-Windows-RelWithDebInfo", zip);
        assert_eq!(fs::read(cache.join(&hash).join("MarioKart64Recompiled-v0.9.2-Windows-RelWithDebInfo.zip")).unwrap(), zip);
        assert_eq!(status(&cache).entries[0].filename, "MarioKart64Recompiled-v0.9.2-Windows-RelWithDebInfo.zip");
        // A name that says already, or contents that don't (a bare program), stay as they are.
        put(tmp.path(), &uses(&cache, "a", "1"), "Game.AppImage", b"\x7FELF an AppImage");
        put(tmp.path(), &uses(&cache, "b", "1"), "tool", b"#!/bin/sh");
        let names: HashSet<String> = status(&cache).entries.into_iter().map(|e| e.filename).collect();
        assert!(names.contains("Game.AppImage") && names.contains("tool"));
    }

    #[test]
    fn one_kept_before_without_its_extension_is_named_for_it_when_the_cache_is_read() {
        let (tmp, cache) = setup();
        let zip = b"PK\x03\x04 an old download";
        let hash = put(tmp.path(), &uses(&cache, "a", "1"), "Game-Windows.zip", zip);
        // As an older build left it: the name its host gave it, no extension.
        fs::rename(cache.join(&hash).join("Game-Windows.zip"), cache.join(&hash).join("Game-Windows")).unwrap();
        let mut entries = read_index(&cache);
        entries[0].filename = "Game-Windows".into();
        write_index(&cache, &entries).unwrap();
        assert_eq!(status(&cache).entries[0].filename, "Game-Windows.zip");
        assert!(cache.join(&hash).join("Game-Windows.zip").is_file());
        assert_eq!(read_index(&cache)[0].filename, "Game-Windows.zip");
        // And it still installs from there.
        assert!(take(&uses(&cache, "a", "1"), &hash, &tmp.path().join("again")).is_some());
    }

    #[test]
    fn reads_checksums_as_the_catalog_gives_them() {
        let hex = "AB".repeat(32);
        assert_eq!(hex_of(&format!("sha256:{hex}")), Some(hex.to_lowercase()));
        assert_eq!(hex_of(&hex.to_lowercase()), Some(hex.to_lowercase()));
        assert_eq!(hex_of("md5:abc"), None);
        assert_eq!(hex_of("sha256:../../etc"), None);
    }
}
