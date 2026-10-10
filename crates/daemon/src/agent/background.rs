//! What a Claude Code agent left running in the background (#606): shell
//! commands it ran with `run_in_background`, and its Monitors. Its turn has
//! ended, but it will carry on when they finish, so `wait --idle` waits for
//! them.
//!
//! claude-agent-acp says when one starts (its tool's response names the
//! task) but tells only JetBrains AIR when one ends. Claude Code writes each
//! task's output to `<tasks>/<id>.output` and ends it with
//! `[exited with code N]`, so that line is the end. A Monitor also ends when
//! its timeout runs out. Anything that can't be checked isn't counted: at
//! worst `wait --idle` returns early, as it did before.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct Task {
    pub id: String,
    /// Where its output goes, once known.
    output: Option<PathBuf>,
    /// When it ends at the latest (a Monitor's timeout).
    until: Option<Instant>,
    /// When it started: where its output goes comes in the next update.
    seen: Instant,
}

/// How long a shell task may go without its output named before it's
/// taken as one that can't be checked.
const NAMED_WITHIN: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub(super) struct Background {
    pub tasks: Vec<Task>,
    /// The folder Claude Code writes this session's task output to.
    dir: Option<PathBuf>,
}

impl Background {
    /// A `session/update` from the agent: a task it started, or where one's
    /// output goes.
    pub fn saw(&mut self, u: &Value, now: Instant) {
        let cc = &u["_meta"]["claudeCode"];
        let response = &cc["toolResponse"];
        if let Some(id) = response["backgroundTaskId"].as_str() {
            self.start(id, None, now);
        } else if cc["toolName"] == "Monitor"
            && let Some(id) = response["taskId"].as_str()
        {
            // One that lasts the session has no end to wait for.
            if response["persistent"] != true {
                let ms = response["timeoutMs"].as_u64().unwrap_or(300_000);
                self.start(id, Some(now + Duration::from_millis(ms)), now);
            }
        }
        if self.tasks.iter().any(|t| t.output.is_none()) {
            let text = u.to_string();
            for (id, path) in outputs(&text) {
                self.dir = path.parent().map(Path::to_path_buf);
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                    t.output = Some(path);
                }
            }
        }
    }

    fn start(&mut self, id: &str, until: Option<Instant>, now: Instant) {
        if !self.tasks.iter().any(|t| t.id == id) {
            self.tasks.push(Task { id: id.to_owned(), output: None, until, seen: now });
        }
    }

    /// Drop the tasks that ended, or that can't be checked. Whether any went.
    pub fn prune(&mut self, now: Instant) -> bool {
        let before = self.tasks.len();
        let dir = self.dir.clone();
        self.tasks.retain(|t| {
            if t.until.is_some_and(|u| now >= u) {
                return false;
            }
            let output = t.output.clone().or_else(|| dir.as_ref().map(|d| d.join(format!("{}.output", t.id))));
            match output {
                Some(path) => running(&path).unwrap_or(t.until.is_some()),
                // A Monitor's timeout still bounds it; a shell task's
                // output may not be named yet.
                None => t.until.is_some() || now < t.seen + NAMED_WITHIN,
            }
        });
        self.tasks.len() != before
    }

    pub fn clear(&mut self) {
        self.tasks.clear();
    }

    pub fn ids(&self) -> Vec<&str> {
        self.tasks.iter().map(|t| t.id.as_str()).collect()
    }
}

/// `(id, output)` for each "Command running in background with ID: <id>.
/// Output is being written to: <path>." in `text`.
fn outputs(text: &str) -> Vec<(String, PathBuf)> {
    const ID: &str = "running in background with ID: ";
    const TO: &str = "Output is being written to: ";
    let mut found = vec![];
    let mut rest = text;
    while let Some(i) = rest.find(ID) {
        rest = &rest[i + ID.len()..];
        let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        let Some(j) = rest.find(TO) else { break };
        let after = &rest[j + TO.len()..];
        if let Some(end) = after.find(".output") {
            found.push((id, PathBuf::from(&after[..end + ".output".len()])));
        }
    }
    found
}

/// Whether the task writing `path` still runs: its file is there and its
/// last line isn't Claude Code's `[exited with code N]` (or another end it
/// writes the same way). `None` when the file can't be read.
fn running(path: &Path) -> Option<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(256))).ok()?;
    let mut bytes = vec![];
    f.take(256).read_to_end(&mut bytes).ok()?;
    let tail = String::from_utf8_lossy(&bytes);
    let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let ended = ["[exited", "[killed", "[stopped", "[failed", "[timed out"].iter().any(|p| last.starts_with(p));
    Some(!(ended && last.ends_with(']')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arugula-bg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_background_shell_runs_until_its_output_says_it_exited() {
        let dir = scratch("shell");
        let path = dir.join("b1.output");
        std::fs::write(&path, "building\n").unwrap();
        let now = Instant::now();
        let mut bg = Background::default();
        bg.saw(
            &json!({ "_meta": { "claudeCode": { "toolName": "Bash", "toolResponse": { "backgroundTaskId": "b1" } } } }),
            now,
        );
        let text = format!(
            "Command running in background with ID: b1. Output is being written to: {}. You will be notified when it completes.",
            path.display()
        );
        bg.saw(&json!({ "_meta": { "terminal_output": { "data": text } } }), now);
        assert!(!bg.prune(now));
        assert_eq!(bg.ids(), ["b1"]);
        std::fs::write(&path, "building\ndone\n\n[exited with code 0]\n").unwrap();
        assert!(bg.prune(now));
        assert!(bg.ids().is_empty());
    }

    #[test]
    fn a_monitor_ends_with_its_output_or_its_timeout() {
        let dir = scratch("monitor");
        let now = Instant::now();
        let mut bg = Background { dir: Some(dir.clone()), ..Default::default() };
        let monitor = |id: &str| json!({ "_meta": { "claudeCode": { "toolName": "Monitor", "toolResponse": { "taskId": id, "timeoutMs": 30000, "persistent": false } } } });
        bg.saw(&monitor("m1"), now);
        bg.saw(&monitor("m2"), now);
        std::fs::write(dir.join("m1.output"), "tick 1\n").unwrap();
        std::fs::write(dir.join("m2.output"), "tick 1\n\n[exited with code 0]\n").unwrap();
        bg.prune(now);
        assert_eq!(bg.ids(), ["m1"]);
        bg.prune(now + Duration::from_secs(31));
        assert!(bg.ids().is_empty());
    }

    #[test]
    fn what_cant_be_checked_isnt_waited_for() {
        let now = Instant::now();
        let mut bg = Background::default();
        // A shell task whose output was never named.
        bg.saw(&json!({ "_meta": { "claudeCode": { "toolResponse": { "backgroundTaskId": "b1" } } } }), now);
        // A Monitor for the whole session.
        bg.saw(&json!({ "_meta": { "claudeCode": { "toolName": "Monitor", "toolResponse": { "taskId": "m1", "persistent": true } } } }), now);
        bg.prune(now);
        assert_eq!(bg.ids(), ["b1"], "its output may yet be named");
        bg.prune(now + NAMED_WITHIN);
        assert!(bg.ids().is_empty());
    }
}
