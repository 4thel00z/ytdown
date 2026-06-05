//! Black-box CLI tests: argument handling, help, exit codes.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_lists_subcommands() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("completions"));
}

#[test]
fn completions_emit_bash_script() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ytdown"));
}

#[test]
fn info_unsupported_url_exits_1() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .args(["info", "https://example.com/x"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no extractor supports"));
}

#[test]
fn search_help_shows_limit_flag() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--limit"));
}

#[test]
fn get_filters_with_itag_exit_2() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .args([
            "get",
            "-f",
            "22",
            "--max-height",
            "720",
            "https://example.com/x",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be combined"));
}

#[test]
fn get_bad_format_selector_exit_2() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .args(["get", "-f", "bogus", "https://example.com/x"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid format selector"));
}

#[test]
fn get_bad_template_exit_2() {
    Command::cargo_bin("ytdown")
        .unwrap()
        .args(["get", "-o", "{nope}", "https://example.com/x"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown placeholder"));
}
