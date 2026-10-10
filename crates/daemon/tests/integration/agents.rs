//! M6b: agent blocks, as ACP clients, against a scripted fake agent server
//! (`fake_acp.py`): turns, permissions (approve, deny, always, cancel),
//! cost, history, search, push, a reboot (the agent dies with the daemon
//! and the session comes back with `session/resume`), and, under a systemd
//! user manager, a restart with an approval pending that the agent server
//! lives through.
//!
//! Real adapters (Claude Code, Codex, Fountain) are in `agents_real.rs`,
//! which costs money and only runs when asked to.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd;

use std::time::Duration;

use agentd::*;
use serde_json::{Value, json};
#[test]
fn an_agent_block_runs_turns_and_asks_before_it_acts() {
    let d = Daemon::child();
    let id = d.open("hello");
    assert_eq!(d.wait(id, "idle"), "done");
    let s = d.state(id);
    assert_eq!(s["status"], "ready");
    assert_eq!(s["server"]["name"], "fake-acp");
    assert!(entries(&s).iter().any(|e| e["type"] == "agent" && e["text"] == "Hello! I am fake."), "{s}");
    assert_eq!(s["cost"]["total"], 0.01);
    assert_eq!(s["cost"]["last_turn"], 0.01);
    let info = d.get(&format!("/api/blocks/{id}"))["info"].clone();
    assert_eq!(info["type"], "agent");

    // It asks before it runs something; approving runs it.
    d.call(id, "send", json!({ "text": "run touch x" }));
    assert_eq!(d.wait(id, "needs-input"), "needs_input");
    let s = d.state(id);
    let p = &s["pending"][0];
    assert_eq!(
        (p["title"].as_str(), p["tool"].as_str(), p["command"].as_str()),
        (Some("touch x"), Some("Bash"), Some("touch x"))
    );
    let summary = d.get("/api/panes");
    assert!(summary.as_array().unwrap().iter().any(|p| p["id"] == id && p["attention"] == "needs_input"), "{summary}");
    let (status, _) = d.raw(
        "POST",
        &format!("/api/blocks/{id}/call/approve"),
        Some(json!({ "id": p["id"], "option": "allow-with-updates" })),
    );
    assert_eq!(status, 400, "the agent's own allow_always is never picked");
    d.call(id, "approve", json!({ "id": p["id"] }));
    assert_eq!(d.wait(id, "idle"), "done");
    let s = d.state(id);
    let tool = last_tool(&s);
    assert_eq!((tool["status"].as_str(), tool["exit"].as_i64()), (Some("completed"), Some(0)), "{tool}");
    assert_eq!(tool["output"], "\u{1b}[32mran: touch x\u{1b}[0m\r\n", "command output, ANSI and all");
    assert!(s["pending"].as_array().unwrap().is_empty());
    assert!((s["cost"]["last_turn"].as_f64().unwrap() - 0.01).abs() < 1e-9, "per-turn delta of a cumulative cost");

    // Denying, with a reason.
    d.call(id, "send", json!({ "text": "run rm -rf y" }));
    d.wait(id, "needs-input");
    d.call(id, "deny", json!({ "reason": "not that" }));
    d.wait(id, "idle");
    let s = d.state(id);
    assert_eq!(last_tool(&s)["status"], "failed");
    assert!(entries(&s).iter().any(|e| e["text"] == "Denied rm -rf y: not that"), "{s}");

    // "Always": the block remembers it and answers next time itself.
    d.call(id, "send", json!({ "text": "run make" }));
    d.wait(id, "needs-input");
    d.call(id, "approve", json!({ "option": "always" }));
    d.wait(id, "idle");
    assert_eq!(d.state(id)["allow"], json!([{ "tool": "Bash", "title": "make" }]));
    d.call(id, "send", json!({ "text": "run make" }));
    assert_eq!(d.wait(id, "idle"), "done", "never asked");
    assert!(entries(&d.state(id)).iter().any(|e| e["text"] == "Allowed make (always allowed)"));
    d.wait_for("the rule saved in its config", || {
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap()).unwrap();
        saved["panes"][id.to_string()]["config"]["allow"] == json!([{ "tool": "Bash", "title": "make" }])
    });

    // Cancel mid-turn, and with a request open (answered `cancelled`).
    d.call(id, "send", json!({ "text": "slow" }));
    d.wait_for("streaming", || {
        entries(&d.state(id)).iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("tick 1")))
    });
    d.call(id, "cancel", json!({}));
    assert_eq!(d.wait(id, "idle"), "idle");
    assert_eq!(d.state(id)["last_stop"], "cancelled");
    d.call(id, "send", json!({ "text": "run sleep 100" }));
    d.wait(id, "needs-input");
    d.call(id, "cancel", json!({}));
    assert_eq!(d.wait(id, "idle"), "idle");
    let s = d.state(id);
    assert_eq!((s["last_stop"].as_str(), s["pending"].as_array().unwrap().len()), (Some("cancelled"), 0));

    // The transcript as Markdown; tail prints it too.
    let text = d.raw("GET", &format!("/api/panes/{id}/capture"), None).1;
    assert!(text.contains("## You\n\nrun touch x"), "{text}");
    assert!(text.contains("**Ran** `touch x` (completed, exit 0)\n\n```\nran: touch x\n```"), "{text}");
    assert_eq!(d.raw("GET", &format!("/api/panes/{id}/tail"), None).1, text);

    // History has its commands and turns; search finds what it said.
    let h = d.get(&format!("/api/history?pane={id}"));
    let texts: Vec<&str> = h.as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect();
    assert!(texts.contains(&"touch x") && texts.iter().any(|t| t.ends_with(": hello")), "{h}");
    // A refused command never ran: no exit code, so it isn't a failure. A
    // turn isn't a command either.
    let denied = h.as_array().unwrap().iter().find(|c| c["text"] == "rm -rf y").unwrap();
    assert_eq!((denied["kind"].as_str(), denied["exit"].is_null()), (Some("command"), true), "{h}");
    let turn = h.as_array().unwrap().iter().find(|c| c["text"].as_str().is_some_and(|t| t.ends_with(": hello")));
    assert_eq!(turn.map(|t| t["kind"].clone()), Some(json!("agent")), "{h}");
    assert!(turn.unwrap()["exit"].is_null(), "{h}");
    let ran = h.as_array().unwrap().iter().find(|c| c["text"] == "touch x").unwrap();
    assert_eq!((ran["kind"].as_str(), ran["exit"].as_i64()), (Some("command"), Some(0)), "{h}");
    let failed = d.get(&format!("/api/history?pane={id}&failed=1"));
    assert!(failed.as_array().unwrap().is_empty(), "nothing ran and failed: {failed}");
    let hits = d.get("/api/search?re=Hello!%20I%20am");
    assert!(hits.as_array().unwrap().iter().any(|h| h["pane"] == id), "{hits}");

    // Closing it ends the agent server.
    let pid = d.state(id)["pid"].as_u64().unwrap();
    assert!(alive(pid));
    d.post(&format!("/api/panes/{id}/close"), json!({}));
    d.wait_for("the agent server to go", || !alive(pid));
}

#[test]
fn an_agents_tool_calls_are_not_failed_commands_in_history() {
    // A Read that failed is the agent's step, not a shell command with an
    // exit code.
    let d = Daemon::child();
    let id = d.open("read /nope/a.txt");
    assert_eq!(d.wait(id, "idle"), "done");
    let h = d.get(&format!("/api/history?pane={id}"));
    let read = h.as_array().unwrap().iter().find(|c| c["text"] == "Read /nope/a.txt").expect("it is in history");
    assert_eq!(read["kind"], "agent", "{h}");
    assert!(read["exit"].is_null(), "{h}");
    let failed = d.get(&format!("/api/history?pane={id}&failed=1"));
    assert!(failed.as_array().unwrap().is_empty(), "{failed}");
    let agent = d.get(&format!("/api/history?pane={id}&kind=agent"));
    assert!(agent.as_array().unwrap().iter().any(|c| c["text"] == "Read /nope/a.txt"), "{agent}");
    let commands = d.get(&format!("/api/history?pane={id}&kind=command"));
    assert!(commands.as_array().unwrap().is_empty(), "{commands}");
}

#[test]
fn a_block_starts_with_the_rules_and_mode_it_was_given() {
    // #163: a lead pre-authorizes its subagent's tools and mode.
    let d = Daemon::child();
    let config = json!({
        "agent": "acp", "command": ["python3", fake()], "cwd": d.sessions, "prompt": "mode",
        "allow": [{ "tool": "Bash" }], "permission_mode": "acceptEdits",
    });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    assert_eq!(d.wait(id, "idle"), "done");
    let s = d.state(id);
    assert!(entries(&s).iter().any(|e| e["text"] == "Mode: acceptEdits"), "{s}");
    assert_eq!((s["permission_mode"].as_str(), s["allow"].clone()), (Some("acceptEdits"), json!([{ "tool": "Bash" }])));
    d.call(id, "send", json!({ "text": "run cargo test" }));
    assert_eq!(d.wait(id, "idle"), "done", "never asked");
    assert!(entries(&d.state(id)).iter().any(|e| e["text"] == "Allowed cargo test (always allowed)"));

    // A mode the agent doesn't have is said, not swallowed.
    let config = json!({ "agent": "acp", "command": ["python3", fake()], "cwd": d.sessions, "prompt": "mode", "permission_mode": "yolo" });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(id, "idle");
    let s = d.state(id);
    assert!(entries(&s).iter().any(|e| e["text"] == "Couldn't switch to permission mode yolo: Invalid Mode"), "{s}");
    assert!(entries(&s).iter().any(|e| e["text"] == "Mode: default"), "{s}");
}

#[test]
fn a_title_the_person_gave_outranks_the_session_s_and_can_be_cleared() {
    // #629: the adapter titles its session at any time; the person's name
    // for the block stays, and `set_title` changes or clears it.
    let d = Daemon::child();
    let config = json!({
        "agent": "acp", "command": ["python3", fake()], "cwd": d.sessions, "prompt": "hello", "title": "Review lane",
    });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    assert_eq!(d.wait(id, "idle"), "done");
    assert_eq!(d.state(id)["title"], "Review lane");

    // The adapter's update comes after: it doesn't replace it.
    d.call(id, "send", json!({ "text": "retitle Fix the parser" }));
    assert_eq!(d.wait(id, "idle"), "done");
    assert_eq!(d.state(id)["title"], "Review lane");
    let panes = d.get("/api/panes");
    let card = panes.as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(card["title"], "Review lane", "{card}");

    // Renamed, then cleared: the session's title is back.
    d.call(id, "set_title", json!({ "title": "Parser lane" }));
    assert_eq!(d.state(id)["title"], "Parser lane");
    d.call(id, "set_title", json!({ "title": "" }));
    assert_eq!(d.state(id)["title"], "Fix the parser");
    d.call(id, "set_title", json!({ "title": "Again" }));
    d.call(id, "set_title", json!({ "title": null }));
    assert_eq!(d.state(id)["title"], "Fix the parser");

    // It's in the config the block is saved with, so a restart keeps it.
    d.call(id, "set_title", json!({ "title": "Kept" }));
    d.wait_for("the title saved in its config", || {
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap()).unwrap();
        saved["panes"][id.to_string()]["config"]["title"] == "Kept"
    });
}

#[test]
fn a_burst_of_chunks_is_sent_to_clients_once_per_tick() {
    // #713: each publish sends the block's whole state to every client, so a
    // stream of small chunks, with gaps (as Claude Code's), is drawn on the
    // 120 ms tick, not once per chunk.
    let d = Daemon::child();
    let id = d.open("hello");
    assert_eq!(d.wait(id, "idle"), "done");
    let s = d.fixture("agent_burst", "an agent streams 50 chunks, 10 ms apart");
    let mut ws = s.ws();
    ws.until("the block's state", |m| m["type"] == "block" && m["block"] == id);
    let started = std::time::Instant::now();
    d.call(id, "send", json!({ "text": "stream 50 10" }));
    let mut sent = 0;
    let end = loop {
        let m = ws.until("a block state", |m| m["type"] == "block" && m["block"] == id);
        sent += 1;
        if m["state"]["status"] == "ready"
            && entries(&m["state"]).iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("w49")))
        {
            break started.elapsed();
        }
    };
    // Whatever is still on its way.
    while ws.recv(Duration::from_millis(300)).is_some() {
        sent += 1;
    }
    let ticks = end.as_millis() as usize / 120;
    eprintln!("{sent} block states in {end:?}");
    assert!(sent <= ticks + 4, "{sent} block states in {end:?} for 50 chunks");
}

#[test]
fn a_client_that_takes_patches_is_sent_what_changed() {
    // #713: a page that asks for patches gets an agent's new entries, not
    // its whole transcript again, and they add up to the block's state.
    let d = Daemon::child();
    let id = d.open("hello");
    assert_eq!(d.wait(id, "idle"), "done");
    let s = d.fixture("agent_patches", "a client that takes patches follows an agent streaming");
    let mut ws = s.ws();
    let mut state = ws.until("the block's state", |m| m["type"] == "block" && m["block"] == id)["state"].clone();
    ws.send(json!({ "type": "block_patches" }));
    d.call(id, "send", json!({ "text": "stream 20 10" }));
    let (mut patches, mut sent, mut whole) = (0, 0, 0);
    loop {
        let m = ws.until("a block state", |m| m["type"] == "block" && m["block"] == id);
        let next = m["state"].clone();
        if next.get("entries_at").is_some() {
            patches += 1;
            sent += next.to_string().len();
            whole += arugula_proto::block_patch::merge(&state, next.clone()).unwrap().to_string().len();
            assert!(entries(&next).len() <= 2, "a patch sends what changed: {next}");
        }
        state = arugula_proto::block_patch::merge(&state, next).expect("each patch follows the last");
        if state["status"] == "ready"
            && entries(&state).iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("w19")))
        {
            break;
        }
    }
    eprintln!("{patches} patches: {sent} bytes, {whole} bytes whole");
    assert!(patches > 0, "no patches");
    assert_eq!(entries(&state), entries(&d.state(id)));
}

#[test]
fn standing_rules_outlive_the_block_that_made_them() {
    // #166: "Always" for a directory or everywhere is the daemon's, not the
    // block's: the next block checks it, it survives a restart, and
    // forgetting it takes effect at once.
    let mut d = Daemon::child();
    let here = d.sessions.join("repo");
    let below = here.join("crates");
    let elsewhere = d.sessions.with_extension("elsewhere");
    for dir in [&below, &elsewhere] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let open_in = |d: &Daemon, cwd: &std::path::Path, prompt: &str| {
        let config = json!({ "agent": "acp", "command": ["python3", fake()], "cwd": cwd, "prompt": prompt });
        d.open_with(json!({ "type": "agent", "config": config }))
    };
    let asked = |d: &Daemon, id: u64, text: &str| -> bool {
        d.call(id, "send", json!({ "text": text }));
        let asked = d.wait(id, "idle") == "needs_input";
        if asked {
            d.call(id, "deny", json!({}));
            d.wait(id, "idle");
        }
        asked
    };

    let a = open_in(&d, &here, "run make");
    d.wait(a, "needs-input");
    let (status, body) =
        d.raw("POST", &format!("/api/blocks/{a}/call/approve"), Some(json!({ "option": "once", "scope": "cwd" })));
    assert_eq!(status, 400, "a scope goes with always: {body}");
    d.call(a, "approve", json!({ "option": "always", "scope": "cwd" }));
    d.wait(a, "idle");
    let here_s = here.display().to_string();
    assert!(
        entries(&d.state(a))
            .iter()
            .any(|e| e["text"] == format!("Allowed make, and from now on: Bash (any) in {here_s}"))
    );
    assert_eq!(d.state(a)["allow"], json!([]), "not the block's own rule");
    let rules = d.get("/api/rules");
    assert_eq!(rules["rules"][0]["cwd"], json!(here_s), "{rules}");
    assert_eq!(rules["rules"][0]["text"], json!(format!("Bash (any) in {here_s}")));

    // A new block under that directory never asks; one elsewhere does.
    let b = open_in(&d, &below, "run cargo build");
    assert_eq!(d.wait(b, "idle"), "done", "never asked");
    assert!(
        entries(&d.state(b))
            .iter()
            .any(|e| e["text"] == format!("Allowed cargo build (standing rule: Bash (any) in {here_s})")),
        "{}",
        d.state(b)
    );
    let c = open_in(&d, &elsewhere, "hello");
    d.wait(c, "idle");
    assert!(asked(&d, c, "run make"));

    // Everywhere, for a prefix: that command and its arguments, nothing else.
    d.call(c, "send", json!({ "text": "run cargo test" }));
    d.wait(c, "needs-input");
    d.call(c, "approve", json!({ "option": "always", "scope": "everywhere", "prefix": "cargo test" }));
    d.wait(c, "idle");
    assert!(!asked(&d, c, "run cargo test --workspace"));
    assert!(asked(&d, c, "run cargo testify"));
    assert!(asked(&d, c, "run cargo test; rm -rf x"));
    assert_eq!(d.get("/api/rules")["rules"][1]["text"], "Bash cargo test… everywhere");

    // In the daemon's state, not a block's: they survive a restart.
    assert!(std::fs::read_to_string(d.state.join("rules.json")).unwrap().contains("cargo test"));
    d.stop();
    d.start();
    let e = open_in(&d, &elsewhere, "run cargo test -p x");
    assert_eq!(d.wait(e, "idle"), "done", "never asked after a restart");

    // Forgetting one: the next request asks again.
    d.raw("DELETE", "/api/rules/1", None);
    assert_eq!(d.get("/api/rules")["rules"].as_array().unwrap().len(), 1);
    assert!(asked(&d, e, "run cargo test -p y"));
    assert_eq!(d.raw("DELETE", "/api/rules/7", None).0, 404);
    d.raw("DELETE", "/api/rules", None);
    let f = open_in(&d, &below, "hello");
    d.wait(f, "idle");
    assert!(asked(&d, f, "run make"));
    let _ = std::fs::remove_dir_all(&elsewhere);
}

#[test]
fn after_a_reboot_the_transcript_is_back_and_the_session_resumes() {
    let mut d = Daemon::child();
    let id = d.open("remember kestrel");
    d.wait(id, "idle");
    let pid = d.state(id)["pid"].as_u64().unwrap();
    // Mid-turn, with an approval open: then the daemon (and, without
    // systemd, everything it started) goes away, as in a reboot.
    d.call(id, "send", json!({ "text": "run sleep 1" }));
    d.wait(id, "needs-input");
    d.stop();
    d.wait_for("the agent to die with it", || !alive(pid));

    d.start();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    d.wait_for("the session", || d.state(id)["status"] == "ready");
    let s = d.state(id);
    assert_ne!(s["pid"].as_u64(), Some(pid), "a new agent server");
    assert!(s["pending"].as_array().unwrap().is_empty(), "its request died with it");
    let text = d.raw("GET", &format!("/api/panes/{id}/capture"), None).1;
    assert!(text.contains("remember kestrel") && text.contains("Started the agent again"), "{text}");
    assert_eq!(text.matches("## You\n\nremember kestrel").count(), 1, "resumed, not replayed: {text}");
    d.call(id, "send", json!({ "text": "recall" }));
    d.wait(id, "idle");
    assert!(entries(&d.state(id)).iter().any(|e| e["text"] == "You said kestrel."), "the agent's context came back");

    // With policy none it waits for "Resume". (Clients set policies over
    // the WebSocket; here, in the saved layout while the daemon is down.)
    d.stop();
    let path = d.state.join("layout.json");
    let mut saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    saved["panes"][id.to_string()]["policy"] = json!({ "kind": "none" });
    std::fs::write(&path, saved.to_string()).unwrap();
    d.start();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    assert_eq!(d.state(id)["status"], "stopped");
    assert!(d.state(id)["pid"].is_null());
    d.call(id, "resume", json!({}));
    d.wait_for("the session", || d.state(id)["status"] == "ready");
    d.call(id, "send", json!({ "text": "recall" }));
    d.wait(id, "idle");
}

const CUT_PROMPT: &str = "Arugula restarted while you were working and your last turn was cut off. Check what you had started (background shells may still be running), then carry on.";

/// #680: a turn the daemon's restart cut off comes back as a card with
/// Continue, which sends the agent on; nothing is sent without it.
#[test]
fn a_turn_cut_off_by_a_restart_asks_whether_to_continue() {
    let mut d = Daemon::child();
    let id = d.open("hello");
    d.wait(id, "idle");
    d.call(id, "send", json!({ "text": "slow" }));
    d.wait_for("the turn to run", || d.state(id)["status"] == "working");
    d.stop();
    d.start();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    assert_eq!(d.wait(id, "needs-input"), "needs_input");
    let text = d.raw("GET", &format!("/api/panes/{id}/capture"), None).1;
    assert!(
        text.contains("Started the agent again") && text.contains("The daemon restarted during this turn"),
        "{text}"
    );
    let list = d.get("/api/attention");
    let card = list.as_array().unwrap().iter().find(|i| i["pane"] == id).unwrap_or_else(|| panic!("{list}"));
    assert_eq!(card["reason"]["actions"], json!(["continue", "dismiss"]), "{card}");
    assert!(card["reason"]["headline"].as_str().unwrap().contains("cut"), "{card}");
    assert!(!text.contains(CUT_PROMPT), "nothing is sent on its own: {text}");

    d.post("/api/attention/act", json!({ "action": "continue", "pane": id }));
    d.wait_for("the prompt to be sent", || {
        entries(&d.state(id)).iter().any(|e| e["type"] == "user" && e["text"] == CUT_PROMPT)
    });
    d.wait(id, "idle");
    let list = d.get("/api/attention");
    assert!(
        list.as_array().unwrap().iter().all(|i| i["pane"] != id || i["state"] != "needs_input"),
        "the card went: {list}"
    );
}

/// #680: only a real interruption is a card: a block that was idle comes
/// back idle.
#[test]
fn an_idle_agent_comes_back_idle_after_a_restart() {
    let mut d = Daemon::child();
    let id = d.open("hello");
    d.wait(id, "idle");
    d.stop();
    d.start();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    d.wait_for("the session", || d.state(id)["status"] == "ready");
    let text = d.raw("GET", &format!("/api/panes/{id}/capture"), None).1;
    assert!(text.contains("Started the agent again") && !text.contains("The daemon restarted"), "{text}");
    let list = d.get("/api/attention");
    assert!(list.as_array().unwrap().iter().all(|i| i["pane"] != id || i["state"] != "needs_input"), "no card: {list}");
}

/// #680: `arugula wait` rides out a daemon restart and returns what the
/// daemon answers after it; a timeout ends it even if the daemon never
/// comes back.
#[test]
fn wait_reconnects_when_the_daemon_restarts() {
    // Built before the turn starts: a cold build outlasts it.
    let cli = cli_bin();
    let mut d = Daemon::child();
    let id = d.open("hello");
    d.wait(id, "idle");
    d.call(id, "send", json!({ "text": "slow" }));
    d.wait_for("the turn to run", || d.state(id)["status"] == "working");
    let sock = d.sock();
    let wait = |args: &[&str]| {
        let mut c = std::process::Command::new(&cli);
        c.arg("--socket").arg(&sock).args(["wait", &format!("%{id}")]).args(args);
        c.env_remove("ARUGULA_PANE").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        c
    };
    let child = wait(&["--needs-input", "--timeout", "60"]).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(700));
    d.stop();
    std::thread::sleep(Duration::from_millis(700));
    d.start();
    let out = child.wait_with_output().unwrap();
    let (so, se) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{so}{se}");
    assert_eq!(so.trim(), "needs-input", "the real result, from the new daemon");
    assert_eq!(se.matches("the daemon went away; waiting for it").count(), 1, "said once: {se}");

    // Gone for good: it gives up at its own timeout, as a timeout.
    d.stop();
    let out = wait(&["--idle", "--timeout", "1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(124), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("timed out"));
}

#[test]
fn a_turn_whose_prompt_is_held_for_background_work_is_not_working() {
    // #681: claude-agent-acp delivers a turn's result, then holds the answer
    // to `session/prompt` while background subagents live. The block says
    // it's idle, but `wait --idle` waits for the real end (#606), and a
    // cancel gets on to what's queued rather than dropping it.
    let d = Daemon::child();
    let id = open_claude(&d, "held");
    d.wait_for("the turn to be held", || d.state(id)["held"] == true);
    let s = d.state(id);
    assert_eq!(s["attention"], "idle", "{s}");
    assert_eq!(s["status"], "working", "the prompt is still open: {s}");
    assert!(entries(&s).iter().any(|e| e["type"] == "agent" && e["text"] == "All done; the report is above."), "{s}");
    let v = d.get(&format!("/api/panes/{id}/wait?until=idle&timeout=1"));
    assert_eq!(v["result"], "timeout", "{v}");
    d.call(id, "send", json!({ "text": "hello" }));
    d.call(id, "cancel", json!({}));
    d.wait_for("the queued prompt to go", || {
        entries(&d.state(id)).iter().any(|e| e["type"] == "agent" && e["text"] == "Hello! I am fake.")
    });
    assert_eq!(d.wait(id, "idle"), "done");
}

#[test]
fn wait_idle_waits_for_what_an_agent_left_running() {
    // #606: a Claude Code agent that ended its turn with a shell command
    // running in the background carries on when it ends; `wait --idle`
    // waits for that. #700: the turn it wakes for counts as one.
    let d = Daemon::child();
    let id = open_claude(&d, "bg 3");
    d.wait_for("its background task", || d.state(id)["background"].as_array().is_some_and(|b| b.len() == 1));
    assert_eq!(d.state(id)["attention"], "done", "its own turn ended");
    let started = std::time::Instant::now();
    assert_eq!(d.wait(id, "idle"), "done");
    assert!(started.elapsed() >= Duration::from_secs(2), "returned after {:?}", started.elapsed());
    d.wait_for("the turn it woke for", || d.state(id)["turns"] == 2);
    let s = d.state(id);
    assert_eq!(s["background"], json!([]), "{s}");
    assert!(entries(&s).iter().any(|e| e["text"] == "The background task finished."), "{s}");
    assert_eq!(s["recent_turns"][0]["prompt"], "(woken by a background task)", "{s}");
}

/// A claude-agent-acp block whose first prompt is `prompt`.
fn open_claude(d: &Daemon, prompt: &str) -> u64 {
    let name = "FAKE_ACP_NAME=@agentclientprotocol/claude-agent-acp";
    let config =
        json!({ "agent": "acp", "command": ["env", name, "python3", fake()], "cwd": d.sessions, "prompt": prompt });
    d.open_with(json!({ "type": "agent", "config": config }))
}

/// Whether the block is between turns (ready), has heard from its woken
/// cycle, and yet shows working. The woken text matters: the pane's attention
/// follows at the next publish tick (#713), so just after a turn ends a block
/// is ready while its pane still shows that turn working.
fn woken(d: &Daemon, id: u64) -> bool {
    let s = d.state(id);
    let panes = d.get("/api/panes");
    s["status"] == "ready"
        && entries(&s).iter().any(|e| e["text"] == "The background task finished.")
        && panes.as_array().unwrap().iter().any(|p| p["id"] == id && p["attention"] == "working")
}

#[test]
fn an_agent_woken_by_a_background_task_shows_working_then_done() {
    // #681: after the turn, claude-agent-acp runs a cycle of its own, outside
    // any prompt, and closes it with an autonomous-origin usage_update.
    let d = Daemon::child();
    let id = open_claude(&d, "wake");
    d.wait_for("the woken agent to show working", || woken(&d, id));
    assert_eq!(d.wait(id, "idle"), "done");
    // The pane shows it at the next publish tick (#713).
    d.wait_for("the pane to stop showing working", || !woken(&d, id));
    let s = d.state(id);
    assert!(entries(&s).iter().any(|e| e["text"] == "The background task finished."), "{s}");
}

#[test]
fn a_woken_agent_that_never_closes_goes_back_to_idle() {
    let d = Daemon::child_env(&[], &[("ARUGULA_AUTONOMOUS_QUIET_MS", "1500")]);
    let id = open_claude(&d, "wake-open");
    d.wait_for("the woken agent to show working", || woken(&d, id));
    assert_eq!(d.wait(id, "idle"), "done");
    // The pane shows it at the next publish tick (#713).
    d.wait_for("the pane to stop showing working", || !woken(&d, id));
}

#[test]
fn a_prompt_during_a_woken_cycle_is_a_normal_turn() {
    let d = Daemon::child();
    let id = open_claude(&d, "wake-open");
    d.wait_for("the woken agent to show working", || woken(&d, id));
    d.call(id, "send", json!({ "text": "hello" }));
    d.wait_for("the turn to finish", || {
        d.state(id)["status"] == "ready" && entries(&d.state(id)).iter().any(|e| e["text"] == "Hello! I am fake.")
    });
    // Its own turn took over and ended: not still woken (the quiet period is a minute).
    // The pane shows it at the next publish tick (#713).
    d.wait_for("the pane to stop showing working", || !woken(&d, id));
    assert_eq!(d.wait(id, "idle"), "done");
}

#[test]
fn an_agent_that_dies_says_so_and_starts_again_on_send() {
    let d = Daemon::child();
    let id = d.open("crash");
    assert_eq!(d.wait(id, "idle"), "needs_input");
    let s = d.state(id);
    assert_eq!(s["status"], "exited");
    assert!(s["error"].as_str().unwrap().contains("exited with code 3"), "{s}");
    d.call(id, "send", json!({ "text": "hello" }));
    assert_eq!(d.wait(id, "idle"), "done");
    assert_eq!(d.state(id)["status"], "ready");
    // Bad configs are refused up front.
    let (status, err) =
        d.raw("POST", "/api/blocks", Some(json!({ "type": "agent", "config": { "agent": "fountain" } })));
    assert_eq!(status, 400, "{err}");
    let (status, err) =
        d.raw("POST", "/api/blocks", Some(json!({ "type": "agent", "vm": true, "config": { "agent": "claude" } })));
    assert_eq!(status, 400);
    assert!(err.contains("VM"), "{err}");
}

/// The CLI, built for its `arugula agent`.
fn cli_bin() -> std::path::PathBuf {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = std::process::Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

/// #629: `arugula agent --title` names the block.
#[test]
fn the_cli_names_the_block_it_starts() {
    let d = Daemon::child();
    let cmd = format!("python3 {}", fake());
    let mut c = arugula_testkit::command(cli_bin());
    c.arg("--socket").arg(d.sock()).args(["agent", "--acp", &cmd, "--title", "Parser lane", "--cwd"]).arg(&d.sessions);
    c.arg("hello").env_remove("ARUGULA_PANE");
    let out = c.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let id = String::from_utf8_lossy(&out.stdout).trim().trim_start_matches('%').parse::<u64>().unwrap();
    d.wait(id, "idle");
    assert_eq!(d.state(id)["title"], "Parser lane");
}

/// What the fake adapter said `env NAME` was, on its last turn.
fn env_said(d: &Daemon, id: u64) -> String {
    entries(&d.state(id))
        .iter()
        .rev()
        .find_map(|e| e["text"].as_str().and_then(|t| t.strip_prefix("ENV ")).map(str::to_owned))
        .unwrap_or_else(|| panic!("no ENV line: {}", d.state(id)))
}

/// #379: a block's adapter logs in as whoever started it: the CLI's
/// `CLAUDE_CONFIG_DIR`, a pane's, an agent's (its `start_agent`), kept
/// across a restart; and a failed login says which one, and how to log in.
#[test]
fn a_block_uses_the_login_of_whoever_started_it() {
    let mut d = Daemon::child();
    let theirs = d.sessions.join("their-claude");
    let cmd = format!("python3 {}", fake());
    let cli = |dir: Option<&std::path::Path>| {
        let mut c = arugula_testkit::command(cli_bin());
        c.arg("--socket").arg(d.sock()).args(["agent", "--acp", &cmd, "--cwd"]).arg(&d.sessions);
        c.args(["env", "CLAUDE_CONFIG_DIR"]).env_remove("ARUGULA_PANE").env_remove("CLAUDE_CONFIG_DIR");
        if let Some(dir) = dir {
            c.env("CLAUDE_CONFIG_DIR", dir);
        }
        let out = c.output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().trim_start_matches('%').parse::<u64>().unwrap()
    };

    // `arugula agent` from a shell with its own login, and from one without.
    let a = cli(Some(&theirs));
    d.wait(a, "idle");
    assert_eq!(env_said(&d, a), format!("CLAUDE_CONFIG_DIR={}", theirs.display()));
    let plain = cli(None);
    d.wait(plain, "idle");
    assert_eq!(env_said(&d, plain), "CLAUDE_CONFIG_DIR unset", "the daemon's default");

    // *Start an agent…* beside a pane running `CLAUDE_CONFIG_DIR=… claude`
    // (a program the shell started with it; the shell has none). Linux
    // only: macOS shows no other process's environment.
    let sessions = d.sessions.clone();
    let claude =
        |prompt: &str| json!({ "agent": "claude", "command": ["python3", fake()], "cwd": sessions, "prompt": prompt });
    if cfg!(target_os = "linux") {
        let pane_dir = d.sessions.join("pane-claude");
        let pane = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
        let marker = "61.379";
        d.post(
            &format!("/api/panes/{pane}/send"),
            json!({ "text": format!("CLAUDE_CONFIG_DIR={} sleep {marker}", pane_dir.display()), "enter": true }),
        );
        d.wait_for("the pane's program", || {
            let ps = std::process::Command::new("ps").args(["-A", "-o", "args="]).output().unwrap();
            String::from_utf8_lossy(&ps.stdout).lines().any(|l| l.trim() == format!("sleep {marker}"))
        });
        let req =
            json!({ "type": "agent", "config": claude("env CLAUDE_CONFIG_DIR"), "split": pane, "from_pane": pane });
        let b = d.open_with(req);
        d.wait(b, "idle");
        assert_eq!(env_said(&d, b), format!("CLAUDE_CONFIG_DIR={}", pane_dir.display()));
    }

    // An agent that starts another hands it its login (start_agent beside it).
    let mut cfg = claude("env CLAUDE_CONFIG_DIR");
    cfg["claude_config_dir"] = json!(theirs);
    let lead = d.open_with(json!({ "type": "agent", "config": cfg }));
    d.wait(lead, "idle");
    assert_eq!(env_said(&d, lead), format!("CLAUDE_CONFIG_DIR={}", theirs.display()));
    let args = json!({ "agent": "claude", "command": cmd, "prompt": "env CLAUDE_CONFIG_DIR" });
    d.call(lead, "send", json!({ "text": format!("mcp start_agent {args}") }));
    d.wait(lead, "idle");
    let said = entries(&d.state(lead))
        .iter()
        .rev()
        .find_map(|e| e["text"].as_str().and_then(|t| t.strip_prefix("MCP ")).map(str::to_owned))
        .unwrap();
    let r: Value = serde_json::from_str(&said).unwrap();
    let started = r["structuredContent"]["block"].as_u64().unwrap_or_else(|| panic!("{r}"));
    d.wait(started, "idle");
    assert_eq!(env_said(&d, started), format!("CLAUDE_CONFIG_DIR={}", theirs.display()));

    // A restarted daemon starts it with the same login.
    d.stop();
    d.start();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{a}"), None).0 == 200);
    d.wait_for("the session", || d.state(a)["status"] == "ready");
    d.call(a, "send", json!({ "text": "env CLAUDE_CONFIG_DIR" }));
    d.wait(a, "idle");
    assert_eq!(env_said(&d, a), format!("CLAUDE_CONFIG_DIR={}", theirs.display()));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap()).unwrap();
    assert_eq!(saved["panes"][a.to_string()]["config"]["claude_config_dir"], json!(theirs), "kept, as a path");
    assert!(saved["panes"][plain.to_string()]["config"].get("claude_config_dir").is_none(), "{saved}");

    // A failed login says whose, and how to log in to it.
    let mut cfg = claude("expired");
    cfg["claude_config_dir"] = json!(theirs);
    let e = d.open_with(json!({ "type": "agent", "config": cfg }));
    d.wait(e, "idle");
    let s = d.state(e);
    let note = entries(&s)
        .iter()
        .rev()
        .find_map(|e| e["text"].as_str().filter(|t| t.starts_with("The turn failed")).map(str::to_owned))
        .unwrap();
    let dir = theirs.display();
    assert!(note.starts_with("The turn failed: Authentication required. "), "{note}");
    assert!(note.contains(&format!("It used the login in CLAUDE_CONFIG_DIR={dir}")), "{note}");
    assert!(note.contains(&format!("`CLAUDE_CONFIG_DIR={dir} claude`, then /login")), "{note}");
    assert!(s["error"].as_str().unwrap().contains(&format!("CLAUDE_CONFIG_DIR={dir}")), "the card's error too: {s}");
    let e = d.open_with(json!({ "type": "agent", "config": claude("expired") }));
    d.wait(e, "idle");
    let text = d.raw("GET", &format!("/api/panes/{e}/capture"), None).1;
    assert!(text.contains("It used the default login (no CLAUDE_CONFIG_DIR)"), "{text}");
    assert!(text.contains("`env -u CLAUDE_CONFIG_DIR claude`, then /login"), "{text}");
    // Another agent's failure is left as it was.
    let other = d.open_with(json!({ "type": "agent", "config": { "agent": "acp", "command": ["python3", fake()], "cwd": d.sessions, "prompt": "expired" } }));
    d.wait(other, "idle");
    assert_eq!(d.state(other)["error"], "Authentication required");
}

/// Under systemd: a restart with an approval pending. The agent server
/// lives through it (its scope; its pipes in the FD store), and the
/// approval, answered to the new daemon, still works.
#[test]
fn a_restart_mid_turn_keeps_the_agent_and_its_pending_approval() {
    let Some(d) = Daemon::service() else { return };
    let id = d.open("hello");
    d.wait(id, "idle");
    let pid = d.state(id)["pid"].as_u64().unwrap();
    d.call(id, "send", json!({ "text": "run make deploy" }));
    d.wait(id, "needs-input");
    let before = d.state(id)["pending"][0].clone();

    d.restart_service();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    let s = d.state(id);
    assert_eq!(s["pid"].as_u64(), Some(pid), "the same agent server");
    assert_eq!(s["status"], "working", "the turn is still running");
    assert_eq!(s["pending"][0]["id"], before["id"], "the same request");
    assert_eq!(d.wait(id, "needs-input"), "needs_input");

    d.call(id, "approve", json!({ "id": before["id"] }));
    assert_eq!(d.wait(id, "idle"), "done", "the turn from before the restart ended");
    let s = d.state(id);
    assert_eq!(last_tool(&s)["output"], "\u{1b}[32mran: make deploy\u{1b}[0m\r\n");
    assert!(entries(&s).iter().any(|e| e["text"] == "Ran it."));
    // And it carries on.
    d.call(id, "send", json!({ "text": "hello" }));
    d.wait(id, "idle");
    assert_eq!(d.state(id)["turns"], 3);
    assert!(alive(pid));

    // A crash too.
    if let Some(unit) = d.unit() {
        assert!(systemctl(&["kill", "--kill-whom=main", "--signal=SIGKILL", unit]));
        std::thread::sleep(Duration::from_millis(300));
        d.wait_up();
    }
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    assert_eq!(d.state(id)["pid"].as_u64(), Some(pid));
    d.call(id, "send", json!({ "text": "recall" }));
    assert_eq!(d.wait(id, "idle"), "done");
    d.post(&format!("/api/panes/{id}/close"), json!({}));
    d.wait_for("the agent server to go", || !alive(pid));
}

/// M71: an image pasted into an agent block's composer reaches an agent
/// that takes images as an image content block, and one that doesn't as
/// its path; either way the transcript names it and the block serves it,
/// and the log keeps its name, not its data. Another file goes as its
/// path; only the block's own uploads are taken; and a paste into the
/// block (`arugula upload`) sends them.
#[test]
fn an_image_reaches_the_agent_as_an_image_or_a_path() {
    let tmp = std::env::temp_dir().join(format!("ilg-agent-images-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let d = Daemon::child_env(&[], &[("TMPDIR", tmp.to_str().unwrap())]);
    let open = |args: &[&str]| {
        let mut command = vec!["python3".to_owned(), fake()];
        command.extend(args.iter().map(|a| a.to_string()));
        let config = json!({ "agent": "acp", "command": command, "cwd": d.sessions, "prompt": "hello" });
        let id = d.open_with(json!({ "type": "agent", "config": config }));
        d.wait(id, "idle");
        id
    };
    let said =
        |id: u64| entries(&d.state(id)).iter().rev().find(|e| e["type"] == "agent").map(|e| e["text"].clone()).unwrap();
    let last_user = |id: u64| entries(&d.state(id)).into_iter().rev().find(|e| e["type"] == "user").unwrap();

    // An agent that takes images gets one.
    let a = open(&["--images"]);
    let up = d.upload(a, "png", &png());
    d.call(a, "send", json!({ "text": "look", "files": [up] }));
    assert_eq!(d.wait(a, "idle"), "done");
    assert_eq!(said(a), "Saw 1 image(s) ['image/png']; text []");
    let user = last_user(a);
    assert_eq!(user["text"], "look");
    let name = user["images"][0].as_str().unwrap().to_owned();
    assert!(name.ends_with(".png"), "{user}");
    let session = d.state(a)["session_id"].as_str().unwrap().to_owned();
    let sent: Value =
        serde_json::from_slice(&std::fs::read(d.sessions.join(format!("prompt-{session}.json"))).unwrap()).unwrap();
    assert_eq!(sent[1]["data"], PNG_B64, "{sent}");
    assert_eq!(sent[1]["_meta"]["arugula/image"], name);
    let served = d.call(a, "image", json!({ "name": name }));
    assert_eq!((served["mime"].as_str(), served["data"].as_str()), (Some("image/png"), Some(PNG_B64)));
    assert!(!std::path::Path::new(&up).exists(), "the upload went into the block");
    let log = std::fs::read_dir(d.state.join(format!("blocks/{a}")))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("seg-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();
    assert!(log.contains(&name) && !log.contains(PNG_B64), "the log names it");
    assert_eq!(d.raw("POST", &format!("/api/blocks/{a}/call/image"), Some(json!({ "name": "../kind" }))).0, 400);

    // Another file goes as its path; a file that isn't its upload, not at all.
    let notes = d.upload(a, "txt", b"some notes");
    d.call(a, "send", json!({ "text": "look", "files": [notes] }));
    d.wait(a, "idle");
    assert_eq!(last_user(a)["text"], format!("look\n\n{notes}"));
    let (status, body) =
        d.raw("POST", &format!("/api/blocks/{a}/call/send"), Some(json!({ "text": "x", "files": ["/etc/hosts"] })));
    assert_eq!(status, 400, "{body}");

    // One that takes none gets its path, to the copy the block keeps.
    let b = open(&[]);
    let up = d.upload(b, "png", &png());
    d.call(b, "send", json!({ "text": "look", "files": [up] }));
    d.wait(b, "idle");
    let user = last_user(b);
    let kept = d.state.join(format!("blocks/{b}/images/{}", user["images"][0].as_str().unwrap()));
    assert_eq!(said(b), format!("Saw 0 image(s) []; text ['{}']", kept.display()));
    assert_eq!(std::fs::read(&kept).unwrap(), png());

    // A paste into the block sends what it's given.
    let up = d.upload(b, "png", &png());
    let r = d.post(&format!("/api/panes/{b}/paste"), json!({ "paths": [up] }));
    assert_eq!(r, json!({ "pasted": true, "sent": true }));
    d.wait(b, "idle");
    let user = last_user(b);
    assert_eq!((user["text"].as_str(), user["images"].as_array().map(Vec::len)), (Some(""), Some(1)), "{user}");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// #590: an agent started from a workspace member, any ACP agent and not
/// only Claude, is told on each prompt the trailers its turn's commits end
/// with: its session's `Chant-Agent` and the turn's `Chant-Run`, the run
/// the block records in chant's ledger. The transcript shows only what the
/// person said.
#[test]
fn a_member_s_agent_is_told_its_turn_s_run_trailer() {
    use std::os::unix::fs::PermissionsExt;
    let d = Daemon::child();
    // A stand-in chant: each ledger write's arguments and fields, a line.
    let chant = d.sessions.join("chant");
    let calls = d.sessions.join("chant.calls");
    std::fs::write(
        &chant,
        format!(
            "#!/bin/sh\n{{ printf '%s ' \"$@\"; cat; echo; }} >> '{}'\nprintf '{{\"run\":{{\"id\":\"x\"}}}}'\n",
            calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&chant, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = json!({
        "agent": "acp", "command": ["python3", fake()], "cwd": d.sessions, "prompt": "hello",
        "chant": { "root": d.sessions, "member": "app", "agent": "app", "chant": chant },
    });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    assert_eq!(d.wait(id, "idle"), "done");
    let s = d.state(id);
    let run = s["recent_turns"][0]["run"].as_str().unwrap().to_owned();
    assert!(run.starts_with(&format!("arugula-{id}-")), "{s}");
    let session = s["session_id"].as_str().unwrap();
    let sent: Value =
        serde_json::from_slice(&std::fs::read(d.sessions.join(format!("prompt-{session}.json"))).unwrap()).unwrap();
    assert_eq!(sent[0]["text"], "hello", "{sent}");
    assert_eq!(sent[1]["_meta"]["arugula/chantRun"], run.as_str(), "{sent}");
    let note = sent[1]["text"].as_str().unwrap();
    assert!(note.ends_with(&format!("\n\nChant-Agent: app\nChant-Run: {run}")), "{note}");
    let user = entries(&s).into_iter().find(|e| e["type"] == "user").unwrap();
    assert_eq!(user["text"], "hello", "the trailers are for the agent");

    // The ledger's start and end are that run.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let ledger = loop {
        let l = std::fs::read_to_string(&calls).unwrap_or_default();
        if l.contains(&format!("workspace runs end {run} --from -")) || std::time::Instant::now() > deadline {
            break l;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let start = ledger.lines().find(|l| l.starts_with("workspace runs start --from -")).expect(&ledger);
    assert!(start.contains(&format!("\"id\":\"{run}\"")), "{ledger}");
    assert!(ledger.contains(&format!("workspace runs end {run} --from -")), "{ledger}");

    // The next turn is another run, named on its own prompt.
    d.call(id, "send", json!({ "text": "again" }));
    assert_eq!(d.wait(id, "idle"), "done");
    let next = d.state(id)["recent_turns"][0]["run"].as_str().unwrap().to_owned();
    assert_ne!(next, run);
    let sent: Value =
        serde_json::from_slice(&std::fs::read(d.sessions.join(format!("prompt-{session}.json"))).unwrap()).unwrap();
    assert_eq!(sent[1]["_meta"]["arugula/chantRun"], next.as_str(), "{sent}");

    // A slash command goes alone, as typed (`/usage` runs only as a prompt
    // of one block; anything after `/compact` is its arguments).
    d.call(id, "send", json!({ "text": "/usage" }));
    assert_eq!(d.wait(id, "idle"), "done");
    let sent: Value =
        serde_json::from_slice(&std::fs::read(d.sessions.join(format!("prompt-{session}.json"))).unwrap()).unwrap();
    assert_eq!(sent, json!([{ "type": "text", "text": "/usage" }]));
}

/// A permission request reaches a subscribed phone as a push with what to
/// approve, and approving by its id (as the notification's action does)
/// works.
#[test]
fn a_permission_request_is_pushed_with_its_approval() {
    let d = Daemon::child();
    let phone = Phone::subscribe(&d);
    let id = d.open("run git push");
    let msg = phone.needs_you();
    assert_eq!(msg["pane"], id);
    assert_eq!(msg["body"], "wants to run git push");
    assert_eq!(msg["approve"]["title"], "git push");
    d.call(id, "approve", json!({ "id": msg["approve"]["id"] }));
    d.wait(id, "idle");
    assert_eq!(last_tool(&d.state(id))["status"], "completed");
}
