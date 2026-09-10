use kova_core::domain::*;
use kova_ops::{enumerate::enumerate_directory, file_ops};
use std::{path::PathBuf, time::Instant};

fn fixture(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kova-{label}-{}", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn long_path_open_rename_refresh_and_undo() {
    let root = fixture("long-path");
    let directory = root
        .join("long-component-".repeat(8))
        .join("another-component-".repeat(7))
        .join("nested");
    std::fs::create_dir_all(&directory).unwrap();
    let before = directory.join("Original.txt");
    assert!(before.as_os_str().len() > 260);
    std::fs::write(&before, b"long path contents").unwrap();
    let after = file_ops::rename(&before, "Renamed.txt").await.unwrap();
    let snapshot = enumerate_directory(Location::new(directory), 1)
        .await
        .unwrap();
    assert_eq!(snapshot.entries[0].path, after);
    let history = kova_platform_windows::undo::History::default();
    history.record(&before, &after, true);
    history.apply(history.next().unwrap().0).unwrap();
    assert_eq!(std::fs::read(before).unwrap(), b"long path contents");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn open_select_rename_refresh_preserves_contents_and_rejects_conflicts() {
    let root = fixture("interaction");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("Before.txt"), b"important contents").unwrap();
    let location = Location::new(root.clone());
    let snapshot = enumerate_directory(location.clone(), 1).await.unwrap();
    let mut selection = SelectionState::empty();
    selection.select_single(0);
    let selected = &snapshot.entries[selection.primary().unwrap()].path;
    let renamed = file_ops::rename(selected, "After.txt").await.unwrap();
    let refreshed = enumerate_directory(location, 2).await.unwrap();
    assert_eq!(refreshed.entries[0].path, renamed);
    assert_eq!(std::fs::read(&renamed).unwrap(), b"important contents");
    std::fs::write(root.join("Existing.txt"), b"untouched").unwrap();
    assert!(file_ops::rename(&renamed, "Existing.txt").await.is_err());
    assert_eq!(
        std::fs::read(root.join("Existing.txt")).unwrap(),
        b"untouched"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn recursive_search_reads_descendants_and_reports_a_missing_root() {
    let root = fixture("recursive");
    std::fs::create_dir_all(root.join("one/two")).unwrap();
    std::fs::write(root.join("one/two/Needle.txt"), b"nested").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let snapshot =
        kova_ops::enumerate::enumerate_tree(Location::new(root.clone()), 7, TabId(1), &tx)
            .await
            .unwrap();
    assert!(snapshot.entries.iter().any(|e| e.name == "Needle.txt"));
    assert!(
        kova_ops::enumerate::enumerate_tree(Location::new(root.join("missing")), 8, TabId(1), &tx)
            .await
            .is_err()
    );
    drop(tx);
    drain.await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// Real filesystem fixture, release-mode only. Does not touch existing user files.
#[tokio::test]
#[ignore = "creates 111,000 temporary files; run in release mode with --nocapture"]
async fn daily_driver_performance() {
    let root = fixture("benchmark");
    std::fs::create_dir(&root).unwrap();
    for count in [1_000, 10_000, 100_000] {
        let directory = root.join(count.to_string());
        std::fs::create_dir(&directory).unwrap();
        for i in 0..count {
            std::fs::write(directory.join(format!("Image {i}.txt")), b"benchmark").unwrap();
        }
        let mut measurements = Vec::new();
        for _ in 0..3 {
            let start = Instant::now();
            let mut snapshot = enumerate_directory(Location::new(directory.clone()), 1)
                .await
                .unwrap();
            let load = start.elapsed();
            let start = Instant::now();
            sort_entries(&mut snapshot.entries, SortDescriptor::by_name());
            let sort = start.elapsed();
            let query = SearchQuery::parse("Image 99 ext:txt", chrono::Local::now());
            let start = Instant::now();
            let matches = snapshot.entries.iter().filter(|e| query.matches(e)).count();
            let search = start.elapsed();
            measurements.push((
                load.as_secs_f64() * 1000.,
                sort.as_secs_f64() * 1000.,
                search.as_secs_f64() * 1000.,
            ));
            assert!(matches > 0);
            assert_eq!(snapshot.entries.len(), count);
        }
        let median = |index: usize| {
            let mut values: Vec<f64> = measurements
                .iter()
                .map(|m| match index {
                    0 => m.0,
                    1 => m.1,
                    _ => m.2,
                })
                .collect();
            values.sort_by(f64::total_cmp);
            values[1]
        };
        println!(
            "KOVA_BENCH entries={count} directory_ms={:.3} natural_sort_ms={:.3} search_match_ms={:.3}",
            median(0),
            median(1),
            median(2)
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
