use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tern_terminal::{CellStyle, LinkKind, Terminal, TerminalOptions, TerminalSnapshot, launch_env};

fn opts(script: &str) -> TerminalOptions {
    TerminalOptions {
        command: Some(vec!["sh".into(), "-c".into(), script.into()]),
        cols: 80,
        rows: 24,
        ..Default::default()
    }
}

fn spawn(options: TerminalOptions) -> Terminal {
    Terminal::spawn(options, Box::new(|| {})).expect("spawn")
}

fn wait_for(
    t: &Terminal,
    what: &str,
    pred: impl Fn(&TerminalSnapshot) -> bool,
) -> TerminalSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snap = t.snapshot();
        if pred(&snap) {
            return snap;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for {what}: {:#?}", texts(&snap));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_exit(t: &Terminal) -> TerminalSnapshot {
    wait_for(t, "exit", |s| s.exited.is_some())
}

fn texts(s: &TerminalSnapshot) -> Vec<String> {
    s.lines.iter().map(|l| l.text()).collect()
}

#[test]
fn prints_and_exits_with_code() {
    let t = spawn(opts("printf hello; exit 3"));
    let snap = wait_exit(&t);
    assert_eq!(snap.lines[0].text(), "hello");
    assert_eq!(snap.exited, Some(3));
    assert_eq!(snap.exit_signal, None);
    assert_eq!((snap.cols, snap.rows), (80, 24));
    assert_eq!(snap.lines.len(), 24);
    assert!(!t.is_running());
    assert_eq!(t.history(), b"hello");
}

#[test]
fn notifications_coalesce_until_snapshot() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let t = Terminal::spawn(
        opts("i=0; while [ $i -lt 3000 ]; do echo line $i; i=$((i+1)); done"),
        Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while t.is_running() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(50));
    // Thousands of reads and an exit, but nobody took a snapshot.
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let snap = t.snapshot();
    assert!(texts(&snap).contains(&"line 2999".to_string()));
    t.write_local(b"\r\n[terminal] Process exited\r\n");
    let deadline = Instant::now() + Duration::from_secs(5);
    while count.load(Ordering::SeqCst) < 2 {
        assert!(Instant::now() < deadline, "no notification after snapshot");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(texts(&t.snapshot()).contains(&"[terminal] Process exited".to_string()));
}

#[test]
fn resize_reaches_the_pty() {
    let t = spawn(opts("read x; stty size; read y"));
    std::thread::sleep(Duration::from_millis(100));
    t.resize(100, 40);
    t.write(b"\n");
    let snap = wait_for(&t, "stty size", |s| texts(s).iter().any(|l| l == "40 100"));
    assert_eq!((snap.cols, snap.rows), (100, 40));
    assert_eq!(snap.lines.len(), 40);
    t.write(b"\n");
    wait_exit(&t);
}

#[test]
fn scrollback_and_scrolling() {
    let mut o = opts("i=1; while [ $i -le 100 ]; do echo $i; i=$((i+1)); done");
    o.rows = 10;
    let t = spawn(o);
    let snap = wait_exit(&t);
    assert_eq!(snap.lines[0].text(), "92");
    assert_eq!(snap.lines[8].text(), "100");
    assert_eq!(snap.display_offset, 0);
    assert_eq!(snap.history_size, 91);

    t.scroll(5);
    let snap = t.snapshot();
    assert_eq!(snap.display_offset, 5);
    assert_eq!(snap.lines[0].text(), "87");
    // The cursor row (101) is below the viewport now.
    assert_eq!(snap.cursor, None);

    t.scroll(1000);
    let snap = t.snapshot();
    assert_eq!(snap.display_offset, 91);
    assert_eq!(snap.lines[0].text(), "1");

    t.scroll_to_bottom();
    assert_eq!(t.snapshot().display_offset, 0);
}

#[test]
fn truecolor_and_indexed_runs() {
    let t = spawn(opts(
        r"printf '\033[38;2;10;20;30mT\033[0m\033[38;5;196mI\033[48;5;21mB\033[0m\033[1;31mR\033[0m\033[2;3;4;7;9mS\033[0m plain'",
    ));
    let snap = wait_exit(&t);
    let runs = &snap.lines[0].runs;
    let style = |fg, bg| CellStyle {
        fg,
        bg,
        ..Default::default()
    };
    assert_eq!(runs[0], ("T".to_string(), style(Some(0xff0a141e), None)));
    assert_eq!(runs[1], ("I".to_string(), style(Some(196), None)));
    assert_eq!(runs[2], ("B".to_string(), style(Some(196), Some(21))));
    assert_eq!(
        runs[3],
        (
            "R".to_string(),
            CellStyle {
                fg: Some(1),
                bold: true,
                ..Default::default()
            }
        )
    );
    assert_eq!(
        runs[4],
        (
            "S".to_string(),
            CellStyle {
                dim: true,
                italic: true,
                underline: true,
                inverse: true,
                strikethrough: true,
                ..Default::default()
            }
        )
    );
    assert_eq!(runs[5], (" plain".to_string(), CellStyle::default()));
    assert_eq!(runs.len(), 6);
    assert_eq!(tern_terminal::color_rgb(0xff0a141e), Err(0x0a141e));
    assert_eq!(tern_terminal::color_rgb(196), Ok(196));
}

#[test]
fn title_and_modes() {
    let t = spawn(opts(
        r"printf '\033]0;my title\007\033[?1h\033[?2004h\033[5 q'",
    ));
    let snap = wait_exit(&t);
    assert_eq!(snap.title.as_deref(), Some("my title"));
    assert!(snap.modes.app_cursor);
    assert!(snap.modes.bracketed_paste);
    assert!(t.app_cursor_mode());
    assert!(t.bracketed_paste());
    assert!(!t.alt_screen());
    assert_eq!(snap.cursor_shape, tern_terminal::CursorShape::Bar);
    assert!(snap.cursor_blinking);

    let t = spawn(opts(r"printf '\033]2;other\033\\'"));
    assert_eq!(wait_exit(&t).title.as_deref(), Some("other"));
}

#[test]
fn keys_and_paste_reach_the_process() {
    let t = spawn(opts(
        r#"read line; printf "got:%s;" "$line"; read l2; printf "p:%s" "$l2""#,
    ));
    t.write(b"hi");
    assert!(t.send_key("enter", false, false, false, false));
    wait_for(&t, "read", |s| {
        s.lines.iter().any(|l| l.text().contains("got:hi;"))
    });
    // Not bracketed: the newline becomes a CR, which ends `read`.
    t.paste("pasted\n");
    let snap = wait_exit(&t);
    assert!(
        texts(&snap).iter().any(|l| l.contains("p:pasted")),
        "{:?}",
        texts(&snap)
    );
}

#[test]
fn env_and_cwd() {
    let dir = std::env::temp_dir();
    let mut o = opts(
        r#"printf '%s|%s|%s|%s|%s|' "$TERM" "$TERN_THREAD_ID" "$TERN_PROJECT_ROOT" "$TERM_PROGRAM" "$COLORTERM"; pwd -P"#,
    );
    o.cwd = dir.clone();
    o.thread_id = Some("thread-9".into());
    o.env = launch_env(std::path::Path::new("/repo"), None);
    o.cols = 200;
    let t = spawn(o);
    let snap = wait_exit(&t);
    let cwd = dir.canonicalize().unwrap();
    assert_eq!(
        snap.lines[0].text(),
        format!(
            "xterm-256color|thread-9|/repo|tern|truecolor|{}",
            cwd.display()
        )
    );
}

#[test]
fn bad_cwd_errors() {
    let mut o = opts("true");
    o.cwd = "/definitely/not/here".into();
    let err = Terminal::spawn(o, Box::new(|| {}))
        .err()
        .unwrap()
        .to_string();
    assert_eq!(
        err,
        "Terminal folder does not exist: /definitely/not/here. Restore the folder at this path before retrying."
    );
    let mut o = opts("true");
    o.cwd = "/etc/passwd".into();
    let err = Terminal::spawn(o, Box::new(|| {}))
        .err()
        .unwrap()
        .to_string();
    assert_eq!(err, "Terminal cwd is not a directory: /etc/passwd");
}

#[test]
fn shell_runs_interactively() {
    let t = spawn(TerminalOptions {
        shell: Some("/bin/sh".into()),
        ..Default::default()
    });
    t.write(b"echo $((6*7)); exit 7\r");
    let snap = wait_exit(&t);
    assert_eq!(snap.exited, Some(7));
    assert!(texts(&snap).iter().any(|l| l == "42"));
}

#[test]
fn close_escalates_to_sigkill() {
    let t = spawn(opts("trap '' HUP TERM; while :; do sleep 0.05; done"));
    std::thread::sleep(Duration::from_millis(200));
    let start = Instant::now();
    t.close();
    let snap = wait_exit(&t);
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(900), "{took:?}");
    assert!(snap.exit_signal.is_some());

    let t = spawn(opts("sleep 30"));
    std::thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    t.close();
    let snap = wait_exit(&t);
    assert!(start.elapsed() < Duration::from_millis(900));
    assert!(snap.exit_signal.is_some());
    // Writes after exit are dropped quietly.
    t.write(b"ignored");
}

#[test]
fn inherited_history_is_replayed() {
    let mut o = opts("printf new");
    o.history = Some(b"old output\r\n\x1b[?2004h".to_vec());
    let t = spawn(o);
    let snap = wait_exit(&t);
    assert_eq!(snap.lines[0].text(), "old output");
    assert_eq!(snap.lines[1].text(), "new");
    // The reset turned bracketed paste back off.
    assert!(!snap.modes.bracketed_paste);
    let history = String::from_utf8(t.history()).unwrap();
    assert!(history.starts_with("old output\r\n"));
    assert!(history.ends_with("new"));
}

#[test]
fn selection_text_word_and_line() {
    let mut o = opts("printf 'hello world foo.bar\\nnext'");
    o.cols = 40;
    let t = spawn(o);
    wait_exit(&t);
    assert_eq!(t.text_in_range((0, 0), (4, 0)), "hello");
    assert_eq!(t.text_in_range((4, 0), (0, 0)), "hello");
    assert_eq!(t.text_in_range((6, 0), (3, 1)), "world foo.bar\nnext");
    assert_eq!(t.word_at((7, 0)), ((6, 0), (10, 0)));
    assert_eq!(t.line_at(0), ((0, 0), (39, 0)));
    assert_eq!(t.all_text(), "hello world foo.bar\nnext");
}

#[test]
fn soft_wrapped_line_and_links() {
    let mut o = opts("printf 'see https://x.io/abcdef and src/a.rs:3\\n'");
    o.cols = 20;
    let t = spawn(o);
    let snap = wait_exit(&t);
    assert!(snap.lines[0].wrapped);
    assert!(!snap.lines[2].wrapped);
    let links = t.links_in_viewport();
    assert_eq!(links.len(), 2, "{links:?}");
    assert_eq!(links[0].kind, LinkKind::Url);
    assert_eq!(links[0].text, "https://x.io/abcdef");
    assert_eq!(links[0].start, (4, 0));
    assert_eq!(links[0].end, (3, 1));
    assert_eq!(links[1].kind, LinkKind::Path);
    assert_eq!(links[1].text, "src/a.rs:3");
    assert_eq!(links[1].start, (8, 1));
    assert_eq!(links[1].end, (18, 1));
}

#[test]
fn osc8_hyperlinks_win() {
    let t = spawn(opts(
        r"printf '\033]8;;https://a.dev/x\033\\click\033]8;;\033\\ https://b.dev'",
    ));
    let snap = wait_exit(&t);
    let spans = &snap.lines[0].hyperlinks;
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].start, spans[0].end), (0, 5));
    assert_eq!(spans[0].uri, "https://a.dev/x");
    let links = t.links_in_viewport();
    assert_eq!(links.len(), 2);
    assert!(links[0].explicit);
    assert_eq!(links[0].text, "https://a.dev/x");
    assert_eq!(links[1].text, "https://b.dev");
}

#[test]
fn wide_chars() {
    let t = spawn(opts("printf '你好x'"));
    let snap = wait_exit(&t);
    assert_eq!(snap.lines[0].text(), "你好x");
    assert_eq!(snap.lines[0].wide, [0, 2]);
}

#[test]
fn answers_device_queries() {
    // DSR cursor position: the reply goes back to the process, which prints it.
    let t = spawn(opts(
        r#"stty raw -echo; printf 'ab\033[6n'; r=$(dd bs=1 count=6 2>/dev/null | od -An -c | tr -d ' \n'); stty sane; printf '\n[%s]' "$r""#,
    ));
    let snap = wait_exit(&t);
    assert!(
        texts(&snap).iter().any(|l| l == r"[033[1;3R]"),
        "{:?}",
        texts(&snap)
    );
}

#[test]
fn clear_empties_grid_and_history() {
    let t = spawn(opts("printf 'a\\nb\\n'"));
    wait_exit(&t);
    t.clear();
    let snap = t.snapshot();
    assert!(snap.lines.iter().all(|l| l.text().is_empty()));
    assert_eq!(snap.history_size, 0);
    assert!(t.history().is_empty());
}

#[test]
fn unfinished_synchronized_update_is_flushed() {
    let t = spawn(opts(r"printf '\033[?2026hsynced'; sleep 2"));
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(t.snapshot().lines[0].text(), "");
    let snap = wait_for(&t, "forced frame", |s| s.lines[0].text() == "synced");
    assert_eq!(snap.exited, None);
}
