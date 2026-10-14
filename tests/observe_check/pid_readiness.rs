use super::*;

#[cfg(unix)]
fn wait_for_positive_pid(path: &Path, deadline: Instant) -> std::io::Result<i32> {
    loop {
        match fs::read_to_string(path) {
            Ok(contents) if !contents.trim().is_empty() => {
                return contents
                    .trim()
                    .parse::<i32>()
                    .ok()
                    .filter(|pid| *pid > 0)
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid child PID")
                    });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child PID was not published before the deadline",
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(unix)]
#[test]
fn pid_readiness_waits_for_contents_after_file_creation() {
    let root = support::temp("pid-publication");
    let path = root.join("child.pid");
    fs::write(&path, "").unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let waiter_path = path.clone();
    let waiter = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        result_tx
            .send(wait_for_positive_pid(
                &waiter_path,
                Instant::now() + Duration::from_secs(5),
            ))
            .unwrap();
    });
    ready_rx.recv().unwrap();
    assert!(matches!(
        result_rx.recv_timeout(Duration::from_millis(50)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    let pid = std::process::id() as i32;
    fs::write(&path, format!("{pid}\n")).unwrap();
    assert_eq!(
        result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap(),
        pid
    );
    waiter.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn pid_readiness_bounds_missing_and_empty_publication() {
    let root = support::temp("pid-deadline");
    let path = root.join("child.pid");
    for contents in [None, Some("")] {
        if let Some(contents) = contents {
            fs::write(&path, contents).unwrap();
        }
        assert_eq!(
            wait_for_positive_pid(&path, Instant::now())
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::TimedOut
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn pid_readiness_refuses_malformed_and_process_group_values() {
    let root = support::temp("pid-invalid");
    let path = root.join("child.pid");
    for contents in ["invalid", "0", "-1"] {
        fs::write(&path, contents).unwrap();
        assert_eq!(
            wait_for_positive_pid(&path, Instant::now() + Duration::from_secs(5))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
    fs::remove_dir_all(root).unwrap();
}
