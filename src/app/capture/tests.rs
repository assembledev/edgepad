use super::*;

#[test]
fn dump_next_command_points_raw_dumps_to_replay_raw() {
    assert_eq!(
        dump_next_command(DumpFormat::Raw, Path::new("bug.raw.ev")),
        "edgepad replay-raw bug.raw.ev"
    );
}

#[test]
fn dump_next_command_points_replay_dumps_to_replay() {
    assert_eq!(
        dump_next_command(DumpFormat::Replay, Path::new("bug.ev")),
        "edgepad replay bug.ev"
    );
}

#[test]
fn dump_no_events_warning_explains_likely_grab() {
    assert_eq!(
            dump_no_events_warning(Path::new("/dev/input/event10")),
            "edgepad dump: no events received from /dev/input/event10 after 3s; device may be grabbed; continuing to wait (stop edgepad.service before capturing)"
        );
}
