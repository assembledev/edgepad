use super::super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ProcessFixture {
    child: Child,
    dir: PathBuf,
}

impl ProcessFixture {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "edgepad-action-process-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&dir).unwrap();
        let args = vec![
            "-c".to_string(),
            "sleep 10 & descendant=$!; printf '%s\\n' \"$descendant\" > \"$1\"; wait".to_string(),
            "edgepad-action-test".to_string(),
            dir.join("child.pid").to_str().unwrap().to_string(),
        ];
        let child = spawn_action_command("sh", &args).unwrap();
        Self { child, dir }
    }

    fn descendant_pid(&self) -> libc::pid_t {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(contents) = fs::read_to_string(self.dir.join("child.pid")) {
                if let Ok(pid) = contents.trim().parse::<libc::pid_t>() {
                    assert!(pid > 1);
                    return pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "test command did not publish its child PID"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for ProcessFixture {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = kill_action_child(&mut self.child, "test command");
            let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn process_is_running(pid: libc::pid_t) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let state = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .trim_start()
                .chars()
                .next()
                .unwrap();
            state != 'Z'
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => false,
        Err(err) => panic!("failed to inspect test child {pid}: {err}"),
    }
}

#[test]
fn shutdown_kills_the_isolated_command_group_including_its_child() {
    let mut fixture = ProcessFixture::start();
    let descendant = fixture.descendant_pid();
    let parent = fixture.child.id() as libc::pid_t;
    assert_eq!(unsafe { libc::getpgid(parent) }, parent);
    assert_ne!(parent, unsafe { libc::getpgrp() });
    assert!(process_is_running(descendant));
    let start = Instant::now();
    let result = interrupt_action_command(&mut fixture.child, "sh");
    assert!(result
        .unwrap_err()
        .contains("interrupted by daemon shutdown"));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(fixture.child.try_wait().unwrap().is_some());
    let deadline = Instant::now() + Duration::from_secs(1);
    while process_is_running(descendant) {
        assert!(
            Instant::now() < deadline,
            "shutdown left the command's child running"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
