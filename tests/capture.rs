#![cfg(unix)]
use easytranslate::capture;
use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn cancellation_stops_and_reaps_capture_process() {
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let trigger = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        signal.store(true, Ordering::Relaxed);
    });
    let start = Instant::now();
    let result = capture::wait(&mut child, || cancelled.load(Ordering::Relaxed));
    trigger.join().unwrap();
    assert!(result.unwrap_err().contains("取消"));
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(child.try_wait().unwrap().is_some());
}

#[test]
fn completed_capture_reports_exit_status_without_touching_desktop() {
    for (program, expected) in [("/usr/bin/true", true), ("/usr/bin/false", false)] {
        let mut child = Command::new(program).spawn().unwrap();
        assert_eq!(
            capture::wait(&mut child, || false).unwrap().success(),
            expected
        );
    }
}
