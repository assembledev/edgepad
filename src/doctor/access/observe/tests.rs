use super::*;

#[test]
fn parses_current_tags_from_udevadm_properties() {
    let tags = parse_udev_current_tags(
        "DEVNAME=/dev/input/event7\nTAGS=:seat:uaccess:\nCURRENT_TAGS=:uaccess:seat:\n",
    );

    assert_eq!(tags, vec!["uaccess", "seat"]);
}

#[test]
fn uses_static_tags_when_current_tags_are_absent() {
    let tags = parse_udev_current_tags("DEVNAME=/dev/uinput\nTAGS=:seat:uaccess:\n");

    assert_eq!(tags, vec!["seat", "uaccess"]);
}

#[test]
fn empty_current_tags_do_not_restore_static_uaccess_tags() {
    assert!(parse_udev_current_tags("TAGS=:seat:uaccess:\nCURRENT_TAGS=\n").is_empty());
}

#[test]
fn detects_named_user_rw_acl() {
    assert!(acl_output_grants_user(
        "user::rw-\nuser:use:rw-\ngroup::---\n",
        "use"
    ));
    assert!(!acl_output_grants_user(
        "user::rw-\nuser:use:r--\ngroup::---\n",
        "use"
    ));
}
