//! `oxi server` end to end on Windows: install the sign-in service, let phones in, connect
//! like the Android app (right and wrong token), uninstall. It changes the user's registry
//! and config, so it is ignored by default; CI's windows-check runs it on a throwaway runner:
//! `cargo test --test server_service -- --ignored`.
#![cfg(windows)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, Instant};

fn oxi(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_oxi"))
        .args(args)
        .output()
        .expect("run oxi");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "oxi {args:?} failed: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

fn run_key_has_oxi() -> bool {
    Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "oxi server",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn phone_token() -> String {
    let path = dirs::config_dir().unwrap().join("oxi").join("phone.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("phone.json")).unwrap();
    config["token"].as_str().unwrap().to_string()
}

/// Send a hello, then a ping; the replies, or `None` once the server hangs up.
fn talk(token: &str) -> Option<(String, String)> {
    let stream = TcpStream::connect(("127.0.0.1", 47811)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let mut writer = stream.try_clone().ok()?;
    let mut reader = BufReader::new(stream);
    let mut read = || {
        let mut line = String::new();
        (reader.read_line(&mut line).ok()? > 0).then_some(line)
    };
    writeln!(
        writer,
        r#"{{"id":1,"method":"daemon.hello","params":{{"token":"{token}"}}}}"#
    )
    .ok()?;
    let hello = read()?;
    writeln!(writer, r#"{{"id":2,"method":"ping","params":{{}}}}"#).ok()?;
    Some((hello, read()?))
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let started = Instant::now();
    while !check() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
#[ignore = "installs the service for this user; run on a throwaway machine"]
fn install_pair_and_uninstall_on_windows() {
    let installed = oxi(&["server", "install"]);
    assert!(installed.contains("oxi server installed"), "{installed}");
    assert!(run_key_has_oxi(), "no Run entry after install");
    eventually("the server runs", || {
        oxi(&["server"]).contains("Server:   running")
    });

    let phone = oxi(&["server", "phone"]);
    assert!(phone.contains("Phones:   on, port 47811"), "{phone}");
    let token = phone_token();

    // Loopback counts as this machine, like a Tailscale peer: the token decides.
    eventually("phones are accepted", || talk(&token).is_some());
    let (hello, ping) = talk(&token).unwrap();
    assert!(hello.contains(r#""daemon":true"#), "{hello}");
    assert!(ping.contains(r#""id":2"#), "{ping}");
    assert!(talk("wrong-token").is_none(), "a wrong token got in");

    let off = oxi(&["server", "phone", "--off"]);
    assert!(off.contains("Phone access is off"), "{off}");

    let removed = oxi(&["server", "uninstall"]);
    assert!(removed.contains("oxi server removed"), "{removed}");
    assert!(!run_key_has_oxi(), "Run entry left after uninstall");
    eventually("the server stopped", || {
        oxi(&["server"]).contains("Server:   not running")
    });
}
