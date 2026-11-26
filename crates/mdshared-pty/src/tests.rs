//! The tests start real programs (`cmd.exe`) in real pseudo-terminals, so they are Windows
//! only; the parts that are plain arithmetic (quoting, the environment block, finding a
//! program) are tested without one. Nothing here is run on Unix.

use super::*;

#[test]
fn a_command_is_a_plain_value_built_in_order() {
    let c = Command::new("prog")
        .arg("a")
        .args(["b", "c"])
        .cwd("dir")
        .env("K", "v")
        .env_remove("GONE");
    assert_eq!(c.program(), "prog");
    assert_eq!(c.args, ["a", "b", "c"]);
    assert_eq!(c.cwd.as_deref(), Some(Path::new("dir")));
    assert_eq!(c.env.len(), 2);
    assert_eq!(c.env[1], (OsString::from("GONE"), None));
    assert_eq!(c.clone(), c);
}

#[test]
fn only_exit_code_zero_is_success() {
    assert!(
        Exit {
            code: Some(0),
            signal: None
        }
        .success()
    );
    assert!(
        !Exit {
            code: Some(1),
            signal: None
        }
        .success()
    );
    assert!(
        !Exit {
            code: None,
            signal: Some(9)
        }
        .success()
    );
}

#[test]
fn a_program_that_is_not_there_is_not_found() {
    let e = Pty::spawn(&Command::new("no-such-program-4f1c9"), 80, 24, || {}).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
    let e = Pty::spawn(&Command::new(""), 80, 24, || {}).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
}

#[cfg(windows)]
mod on_windows {
    use super::super::windows::{env_block, environment, quoted, resolve};
    use super::*;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    const LIMIT: Duration = Duration::from_secs(30);

    /// Starts a command and gives the terminal with the receiving end of its wakes.
    fn start(command: Command) -> (Pty, Receiver<()>) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let pty = Pty::spawn(&command, 80, 24, move || {
            let _ = tx.lock().unwrap().send(());
        })
        .expect("the command starts");
        (pty, rx)
    }

    /// Runs until the program has ended and gives all it wrote, as text, and how it ended.
    /// `on_output` sees the text so far after every batch and may write to the program.
    fn finish(
        mut pty: Pty,
        wakes: Receiver<()>,
        mut on_output: impl FnMut(&Pty, &str),
    ) -> (String, Exit) {
        let began = Instant::now();
        let mut all = Vec::new();
        loop {
            let _ = wakes.recv_timeout(Duration::from_millis(200));
            let batch = pty.take_output();
            if !batch.is_empty() {
                all.extend(batch);
                on_output(&pty, &String::from_utf8_lossy(&all));
            }
            if let Some(exit) = pty.exit() {
                return (String::from_utf8_lossy(&all).into_owned(), exit);
            }
            assert!(
                began.elapsed() < LIMIT,
                "the program did not end; output so far: {:?}",
                String::from_utf8_lossy(&all)
            );
        }
    }

    fn cmd(script: &str) -> Command {
        Command::new("cmd.exe").args(["/d", "/c", script])
    }

    #[test]
    fn what_a_program_prints_comes_back_and_its_end_is_told_after_it() {
        let (pty, wakes) = start(cmd("echo hello from the other side"));
        assert!(pty.pid() > 0);
        let (text, exit) = finish(pty, wakes, |_, _| {});
        assert!(text.contains("hello from the other side"), "{text:?}");
        assert!(exit.success());
        assert_eq!(exit.signal, None);
    }

    #[test]
    fn the_exit_code_is_the_programs() {
        let (pty, wakes) = start(cmd("exit 7"));
        let (_, exit) = finish(pty, wakes, |_, _| {});
        assert_eq!(exit.code, Some(7));
        assert!(!exit.success());
    }

    #[test]
    fn what_is_written_reaches_the_program_as_typed_keys() {
        let (pty, wakes) = start(cmd("set /p X=name? & call echo got [%X%]"));
        let mut sent = false;
        let (text, exit) = finish(pty, wakes, |pty, text| {
            if !sent && text.contains("name?") {
                sent = true;
                pty.write(b"").unwrap();
                pty.write(b"pty\r").unwrap();
            }
        });
        assert!(sent, "the prompt never came: {text:?}");
        assert!(text.contains("got [pty]"), "{text:?}");
        assert!(exit.success());
    }

    #[test]
    fn the_environment_and_the_folder_are_the_commands() {
        let dir = std::env::temp_dir();
        let c = cmd("echo [%MDS_PTY_SET%][%MDS_PTY_GONE%] & cd")
            .cwd(&dir)
            .env("MDS_PTY_SET", "yes");
        // SAFETY: no other thread of the tests reads or writes this variable.
        unsafe { std::env::set_var("MDS_PTY_GONE", "still here") };
        let (pty, wakes) = start(c.env_remove("mds_pty_gone"));
        let (text, _) = finish(pty, wakes, |_, _| {});
        // An unset variable is left as its own name by cmd's echo.
        assert!(text.contains("[yes][%MDS_PTY_GONE%]"), "{text:?}");
        let folder = dir.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(text.to_lowercase().contains(&folder), "{text:?}");
    }

    #[test]
    fn the_program_sees_the_size_and_a_new_one() {
        let (tx, wakes) = channel();
        let tx = Mutex::new(tx);
        let mut pty = Pty::spawn(
            &cmd("mode con & pause >nul & mode con"),
            97,
            31,
            move || {
                let _ = tx.lock().unwrap().send(());
            },
        )
        .unwrap();
        assert_eq!(pty.size(), (97, 31));
        let began = Instant::now();
        let mut all = String::new();
        let mut resized = false;
        loop {
            let _ = wakes.recv_timeout(Duration::from_millis(200));
            all.push_str(&String::from_utf8_lossy(&pty.take_output()));
            // `mode con` prints "Columns:" and the number; the first report ends before the
            // pause.
            if !resized && all.contains("97") && all.contains("31") {
                resized = true;
                pty.resize(97, 31).unwrap();
                pty.resize(0, 0).unwrap();
                assert_eq!(pty.size(), (1, 1));
                pty.resize(113, 37).unwrap();
                assert_eq!(pty.size(), (113, 37));
                pty.write(b" ").unwrap();
            }
            if pty.exit().is_some() {
                break;
            }
            assert!(began.elapsed() < LIMIT, "no end; output so far: {all:?}");
        }
        assert!(resized, "the first size never showed: {all:?}");
        assert!(all.contains("113") && all.contains("37"), "{all:?}");
        assert!(
            pty.resize(80, 24).is_err(),
            "a closed terminal cannot be resized"
        );
    }

    #[test]
    fn a_killed_program_ends_and_killing_twice_is_nothing() {
        let (pty, wakes) = start(cmd("pause"));
        assert!(pty.running());
        pty.kill().unwrap();
        let (_, exit) = finish(pty, wakes, |pty, _| {
            let _ = pty.kill();
        });
        assert_eq!(exit.code, Some(1));
    }

    #[test]
    fn dropping_the_terminal_ends_the_program() {
        let (pty, _wakes) = start(cmd("pause"));
        let pid = pty.pid();
        drop(pty);
        let began = Instant::now();
        while process_exists(pid) {
            assert!(began.elapsed() < LIMIT, "the program outlived its terminal");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn process_exists(pid: u32) -> bool {
        let out = std::process::Command::new("tasklist")
            .args(["/fo", "csv", "/nh", "/fi", &format!("PID eq {pid}")])
            .output();
        out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    }

    #[test]
    fn a_flood_of_output_is_held_at_the_high_water_mark_and_all_of_it_arrives() {
        // About 9 MB: more than twice the mark. Nothing is taken for a while, then all of it.
        let (mut pty, wakes) = start(cmd(
            "for /L %i in (1,1,60000) do @echo line %i of a long, long run of output that fills the buffer up to its mark",
        ));
        std::thread::sleep(Duration::from_millis(1500));
        let held = pty.shared.lock().out.len();
        assert!(held <= HIGH_WATER + 64 * 1024, "{held} bytes were kept");
        let began = Instant::now();
        let mut all = Vec::new();
        loop {
            let _ = wakes.recv_timeout(Duration::from_millis(200));
            all.extend(pty.take_output());
            if pty.exit().is_some() {
                break;
            }
            assert!(
                began.elapsed() < Duration::from_secs(120),
                "the flood did not end"
            );
        }
        let text = String::from_utf8_lossy(&all);
        assert!(text.contains("line 1 of"), "the start is missing");
        assert!(text.contains("line 60000 of"), "the end is missing");
    }

    #[test]
    fn a_screen_on_the_other_side_shows_what_the_program_wrote() {
        use mdshared_term::Screen;
        let (mut pty, wakes) = start(cmd("echo first & echo second"));
        let mut screen = Screen::new(80, 24, 100);
        let began = Instant::now();
        loop {
            let _ = wakes.recv_timeout(Duration::from_millis(200));
            screen.feed(&pty.take_output());
            pty.write(&screen.take_reply()).unwrap_or(());
            if pty.exit().is_some() {
                break;
            }
            assert!(began.elapsed() < LIMIT);
        }
        let rows: Vec<String> = (0..24).map(|r| screen.row_text(r)).collect();
        assert!(rows.iter().any(|r| r.trim() == "first"), "{rows:?}");
        assert!(rows.iter().any(|r| r.trim() == "second"), "{rows:?}");
    }

    #[test]
    fn a_batch_file_runs_through_the_shell_and_unsafe_arguments_are_refused() {
        let dir = std::env::temp_dir().join(format!("mds-pty-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("say it.cmd");
        std::fs::write(&file, "@echo off\r\necho said [%~1] [%~2]\r\n").unwrap();
        let (pty, wakes) = start(Command::new(&file).args(["one two", "three"]));
        let (text, exit) = finish(pty, wakes, |_, _| {});
        assert!(text.contains("said [one two] [three]"), "{text:?}");
        assert!(exit.success());
        for bad in ["a\"b", "100%", "two\nlines"] {
            let e = Pty::spawn(&Command::new(&file).arg(bad), 80, 24, || {}).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_nul_inside_an_argument_is_refused() {
        let e = Pty::spawn(&Command::new("cmd.exe").arg("a\0b"), 80, 24, || {}).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn arguments_are_quoted_as_the_c_runtime_reads_them_back() {
        assert_eq!(quoted(&["prog", "plain", "-x=1"]), "prog plain -x=1");
        assert_eq!(quoted(&["a b"]), "\"a b\"");
        assert_eq!(quoted(&[""]), "\"\"");
        assert_eq!(quoted(&["tab\there"]), "\"tab\there\"");
        assert_eq!(quoted(&["say \"hi\""]), "\"say \\\"hi\\\"\"");
        // Backslashes are literal unless a quote follows them.
        assert_eq!(quoted(&["C:\\dir\\file"]), "C:\\dir\\file");
        assert_eq!(quoted(&["C:\\my dir\\"]), "\"C:\\my dir\\\\\"");
        assert_eq!(quoted(&["a\\\"b"]), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn the_environment_block_is_sorted_changed_and_doubly_ended() {
        let changes = vec![
            (OsString::from("zz_mds_new"), Some(OsString::from("1"))),
            (OsString::from("PATH"), Some(OsString::from("C:\\only"))),
            (OsString::from("Zz_Mds_New"), Some(OsString::from("2"))),
            (OsString::from("SystemRoot"), None),
        ];
        let env = environment(&changes);
        assert_eq!(env["PATH"].1, "C:\\only");
        // Names differ only by case: the later one wins, with its own spelling.
        assert_eq!(
            env["ZZ_MDS_NEW"],
            (OsString::from("Zz_Mds_New"), OsString::from("2"))
        );
        assert!(!env.contains_key("SYSTEMROOT"));
        let block = env_block(&env).unwrap();
        assert_eq!(&block[block.len() - 2..], [0, 0]);
        let text = String::from_utf16_lossy(&block);
        let names: Vec<String> = text
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(|s| s.split('=').next().unwrap_or_default().to_uppercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        // Not assert_eq: a failure must not print the caller's environment.
        assert!(names == sorted, "the names are not in order");
        assert_eq!(env_block(&Default::default()).unwrap(), [0, 0]);
    }

    #[test]
    fn a_program_is_found_by_name_on_the_path_with_the_usual_extensions() {
        let dir = std::env::temp_dir().join(format!("mds-pty-resolve-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("tool.cmd"), "").unwrap();
        std::fs::write(dir.join("tool.exe"), "").unwrap();
        std::fs::write(sub.join("other.bat"), "").unwrap();
        std::fs::write(dir.join("readme.txt"), "").unwrap();
        let path = std::env::join_paths([dir.join("missing"), dir.clone(), sub.clone()]).unwrap();
        let find = |name: &str, cwd: Option<&Path>| resolve(OsStr::new(name), &path, cwd);
        assert_eq!(find("tool", None).unwrap(), dir.join("tool.exe"));
        assert_eq!(find("tool.cmd", None).unwrap(), dir.join("tool.cmd"));
        assert_eq!(find("other", None).unwrap(), sub.join("other.bat"));
        assert_eq!(find("readme.txt", None).unwrap(), dir.join("readme.txt"));
        assert_eq!(
            find("nothing", None).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        // With a folder in the name the path is not searched.
        assert_eq!(
            find("sub\\other", Some(&dir)).unwrap(),
            sub.join("other.bat")
        );
        assert_eq!(
            find("sub\\tool", Some(&dir)).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            find(dir.join("tool").to_str().unwrap(), None).unwrap(),
            dir.join("tool.exe")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
