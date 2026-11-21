//! Content declarations admitted (or refused) within a stable release line.
//! To add an entry, create its corpus folder and run this test with
//! `SUNRISE_BLESS_CONTENT_GOLDEN=1`. Do not re-bless existing entries to fix a mismatch.

use std::{collections::BTreeSet, env, fs, io, path::Path};
use sunrise_mod_package::{Content, Package};

const CORPUS: &str = "tests/content-corpus";
const GOLDEN: &str = "tests/content-golden";

#[test]
fn content_golden() {
    // Cargo runs integration tests from the crate root, even when invoked with -p
    // from the workspace. Keep the literal forward slashes in Content's Source::Dir.
    let entries = entry_names(CORPUS, true);
    let goldens = entry_names(GOLDEN, false);
    let bless = env::var("SUNRISE_BLESS_CONTENT_GOLDEN").as_deref() == Ok("1");
    let orphans: Vec<_> = goldens.difference(&entries).collect();
    let mut mismatches = Vec::new();
    let mut first_diff = None;

    for entry in &entries {
        let path = format!("{CORPUS}/{entry}");
        let actual = dump(Path::new(&path));
        let golden = format!("{GOLDEN}/{entry}.txt");
        if bless {
            fs::write(&golden, actual).unwrap_or_else(|e| panic!("{golden}: {e}"));
            continue;
        }
        let expected = match fs::read(&golden) {
            Ok(bytes) if bytes == actual.as_bytes() => continue,
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => "(missing)\n".into(),
            Err(e) => panic!("{golden}: {e}"),
        };
        mismatches.push(format!("{entry}\n"));
        first_diff.get_or_insert_with(|| {
            format!("{entry} changed\n--- golden ---\n{expected}--- actual ---\n{actual}")
        });
    }

    if let Some(path) = env::var_os("SUNRISE_CONTENT_GOLDEN_REPORT") {
        let path = Path::new(&path);
        fs::write(path, mismatches.concat()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    assert!(!entries.is_empty(), "{CORPUS} is empty");
    assert!(
        orphans.is_empty(),
        "goldens without corpus entries: {orphans:?}"
    );
    if let Some(diff) = first_diff {
        panic!(
            "content golden mismatches:\n{}\n{diff}",
            mismatches.concat()
        );
    }
}

#[test]
fn content_golden_is_deterministic() {
    for entry in entry_names(CORPUS, true) {
        let path = format!("{CORPUS}/{entry}");
        let path = Path::new(&path);
        assert_eq!(dump(path), dump(path), "{entry} changed between reads");
    }
}

fn entry_names(dir: &str, corpus: bool) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .map(|entry| {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            let filename = entry.file_name().into_string().expect("UTF-8 fixture name");
            let name = if corpus {
                assert!(kind.is_dir(), "{dir}/{filename} is not a directory");
                &filename
            } else {
                assert!(kind.is_file(), "{dir}/{filename} is not a regular file");
                filename
                    .strip_suffix(".txt")
                    .expect("golden file ends in .txt")
            };
            assert!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "invalid fixture name: {dir}/{filename}"
            );
            name.to_owned()
        })
        .collect()
}

fn dump(path: &Path) -> String {
    let Package { manifest, .. } = match Package::read(path) {
        Ok(package) => package,
        Err(error) => return format!("manifest:\nrefused: {error}\n"),
    };
    let content = Content::read(path, &manifest).and_then(|content| {
        match content.server_only_refusal(&manifest) {
            Some(error) => Err(error),
            None => Ok(content),
        }
    });
    let content = match content {
        Ok(content) => format!("{content:#?}"),
        Err(error) => format!("refused: {error}"),
    };
    format!("manifest:\n{manifest:#?}\n\ncontent:\n{content}\n")
}
