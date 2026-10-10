// Generated from crates/proto by `just proto-ts`: don't edit. Change the
// Rust types and run it again; CI checks this file is current.

export type PaneId = number;
export type TabId = number;
export type SessionId = number;
export type NodeId = number;
export type ClientId = number;
export type MachineId = number;

export const CALL_MAX = 5;

/**
 * `POST /api/attention/act`: do something about one pane's reason, or
 * several at once ("allow all 3", "dismiss all 11"). Each pane needs
 * editor on its session.
 */
export type ActRequest = { action: Action, pane?: number, panes?: Array<number>, 
/**
 * The ask it answers (`AskRef::id`); without one, whatever the pane
 * asks now.
 */
id?: string, 
/**
 * `answer`: the card's fields.
 */
content?: Record<string, unknown>, 
/**
 * `allow`: `once` (default) or `always`.
 */
option?: string, 
/**
 * `allow` `always` for Claude Code in a terminal (M29): which of its
 * suggestions to keep (default the first).
 */
suggestion?: number, 
/**
 * `deny`: why, for the agent.
 */
message?: string, 
/**
 * `accept` (M28): the file as it should be saved, when someone
 * changed the proposal first.
 */
text?: string, };

export type ActResponse = { results: Array<ActResult>, };

export type ActResult = { pane: number, ok: boolean, error?: string, };

/**
 * Something done about a reason (`POST /api/attention/act`).
 */
export type Action = "allow" | "deny" | "answer" | "dismiss" | "continue" | "accept" | "reject" | "rerun" | "expire";

/**
 * How much a pane prints (M23).
 */
export type Activity = { 
/**
 * Bytes of output a second, over the last second or so.
 */
bps: number, 
/**
 * When it last printed anything (ms since the epoch); 0: not since
 * the daemon started.
 */
last_ms: number, };

/**
 * The agent working on an issue, and what it made.
 */
export type AgentLink = { branch: string, 
/**
 * The worktree, absolute.
 */
worktree: string, 
/**
 * What the branch was made from.
 */
base: string, 
/**
 * The agent block.
 */
block: number, 
/**
 * `claude`, `codex`, …
 */
agent: string, at_ms: number, 
/**
 * Its pull request, once there is one (found once).
 */
pr?: number, pr_url?: string, 
/**
 * The PR block opened beside the agent.
 */
pr_block?: number, };

/**
 * The open question or approval behind an `ask` reason.
 */
export type AskRef = { 
/**
 * What `allow`, `deny` and `answer` name (a permission request's id, or
 * a question's).
 */
id: string, 
/**
 * `approve` (allow or deny it) or `question` (answer or skip it).
 */
what: AskWhat, 
/**
 * Who asks: the agent (`claude`, `fountain`, …).
 */
agent: string, };

export type AskWhat = "approve" | "question";

export type AttachPane = { pane: number, 
/**
 * Offset just past the last byte the client has, or `None` for a fresh
 * view.
 */
offset: number | null, 
/**
 * At most this many rows of scrollback in a snapshot: what the client
 * keeps (`None`: all of it). After a [`ServerMsg::Resync`], `0`: the
 * client keeps what it has and needs only the screen.
 */
history?: number, };

/**
 * Whether a pane wants you: the cheap version of an "agent block".
 */
export type Attention = "idle" | "working" | "needs_input" | "done";

/**
 * What a block is; where one runs is its `host`, not its type. All types
 * share one id space (`%N`) and one place in the layout tree.
 */
export type BlockType = "terminal" | "browser" | "agent" | "editor" | "diff" | "file" | "remote" | "workspace" | "app" | "forge" | "fountain" | "invite" | "agents";

/**
 * A side of a PR: which repository (a fork's, for its head), branch and
 * commit.
 */
export type Branch = { repo: string | null, branch: string, sha: string, };

/**
 * A huddle: a voice call on a session (M63), peer to peer between its
 * members, signaled through this daemon.
 */
export type Call = { session: number, 
/**
 * New each time a huddle starts on the session, so what's signed for
 * one can't be used in the next.
 */
id: string, 
/**
 * When it started (ms since the epoch).
 */
started: number, 
/**
 * In the order they joined.
 */
members: Array<CallMember>, };

export type CallMember = { client: number, 
/**
 * Their principal id.
 */
who: string, name: string, pic?: string, muted?: boolean, 
/**
 * When they joined (ms since the epoch).
 */
joined: number, 
/**
 * Their device's id, when they connected with a device key (through
 * control): their fingerprints are signed. Absent for a tailnet or
 * local connection.
 */
device?: string, };

/**
 * What one huddle member sends another through the daemon (M63), which
 * passes it on untouched: [`ClientMsg::CallSignal`]'s and
 * [`ServerMsg::CallSignal`]'s `signal`.
 */
export type CallSignal = { type: SdpKind, sdp: string, 
/**
 * Hex Ed25519 signature of [`call_fingerprint_body`] by the sender's
 * device key, when it has one.
 */
sig?: string, };

/**
 * A person's note asking an agent to change its draft.
 */
export type ChangesAsked = { note: string, by: string, at_ms: number, 
/**
 * When the agent drafted again.
 */
revised_ms?: number, };

export type Check = { name: string, source: CheckSource, state: CheckState, allow_failure: boolean, 
/**
 * Absolute (Forgejo's own are relative to the site; the adapter makes
 * them whole).
 */
url: string | null, description: string | null, run: RunRef | null, };

export type CheckSource = "status" | "action" | "check_run" | "pipeline_job";

export type CheckState = "queued" | "running" | "success" | "failure" | "cancelled" | "skipped" | "neutral" | "action_required" | "manual";

export type Child = { weight: number, node: Node, };

/**
 * Control messages from a client.
 */
export type ClientMsg = { "type": "attach", panes: Array<AttachPane>, zstd?: boolean, acks?: boolean, 
/**
 * The client encodes keys for the kitty keyboard protocol, so
 * programs asking may be told it's there (M31).
 */
kitty_keys?: boolean, } | { "type": "ack", pane: number, offset: number, } | { "type": "detach", panes: Array<number>, } | { "type": "view", tab: number, cols: number, rows: number, zoom: number | null, claim: boolean, typed?: boolean, } | { "type": "intent", id: number | null, intent: Intent, } | { "type": "pane", pane: number, op: PaneOp, } | { "type": "focus", pane: number | null, } | { "type": "ping", id: number, } | { "type": "subscribe", summary: boolean, } | { "type": "follow", pane: number, on: boolean, } | { "type": "block_patches" } | { "type": "call_join", session: number, } | { "type": "call_leave", session: number, } | { "type": "call_mute", session: number, muted: boolean, } | { "type": "call_signal", session: number, to: number, signal: CallSignal, } | { "type": "hand", tools: Array<HandTool>, name?: string, } | { "type": "hand_reply", id: number, result?: unknown, error?: string, };

/**
 * A command the shell integration reported.
 */
export type CommandInfo = { text: string | null, cwd: string | null, exit: number | null, started_ms: number, ended_ms: number | null, 
/**
 * Stream offsets of its output: `tail --from start`.
 */
start: number, end: number | null, 
/**
 * Who started it (M13), when someone other than the owner might have.
 */
by?: string, };

/**
 * A machine's standing with Arugula control (#325): whether and where
 * it's joined, whether control is reachable, or that control dropped it.
 * The page, the tray and `arugula status` all show this.
 */
export type ControlState = { 
/**
 * `not_joined`, `joined` or `dropped`.
 */
state: "not_joined" | "joined" | "dropped", 
/**
 * The control it's (or was) joined to.
 */
url?: string, 
/**
 * `account` or `team`.
 */
kind?: "account" | "team", 
/**
 * The team's name, or the account's login (empty until control says).
 */
name?: string, 
/**
 * Joined: reachable through control now (its relay socket is up; for a
 * sandbox behind a provider's proxy, the last refresh worked).
 */
connected: boolean, 
/**
 * When control last answered (ms since the epoch).
 */
seen_ms?: number, 
/**
 * Joined: what last went wrong talking to control, until it works again.
 */
error?: string, 
/**
 * Dropped: what control said ("not an enrolled daemon (left, or
 * revoked?)", or that its key was removed, #330).
 */
said?: string, 
/**
 * Dropped: when this machine first heard it (ms since the epoch).
 */
dropped_ms?: number, 
/**
 * Dropped: a join waiting for approval, its code (a machine whose key
 * was removed asks to join again with a new key by itself, #330; or
 * someone started one), and where a signed-in device approves it.
 */
code?: string, approve?: string, };

/**
 * An editor's debug session.
 */
export type DebugState = { 
/**
 * `running` or `paused`.
 */
state: "running" | "paused", 
/**
 * Why it stopped: `breakpoint`, `exception`, `step`, ...
 */
reason?: string | null, file?: string | null, line?: number | null, };

/**
 * A decision covering a region, as chant's intent read ranks it
 * (`graph --intent <dir>`'s `why.decisions`, #617): chant does the
 * `member:` and `path:` covering and follows supersession.
 */
export type DecisionRef = { id: string, title: string | null, state: string | null, 
/**
 * How it covers the region: `carried`, `path`, `contract`, `issue`,
 * `member`, or `related` (only through supersession or a commit).
 */
relevance: string, 
/**
 * Not superseded.
 */
current: boolean, 
/**
 * In a state its kind closes, such as ratified.
 */
closed: boolean, };

/**
 * Changes to the last [`State`]: each pane in `panes` is `{id, ...}` with
 * only the fields that changed (a field set to `null` went back to its
 * default, absent); `gone` panes left this client's view. `machines` and
 * `presence` (and `threads`) are whole when present. Anything else (sessions, tabs,
 * options, roles) changes with a new `State`.
 */
export type Delta = { panes?: Array<{ id: PaneId } & Partial<PaneInfo>>, gone?: Array<number>, machines?: Array<Machine> | null, presence?: Array<Presence> | null, threads?: Array<ThreadSummary> | null, calls?: Array<Call> | null, };

/**
 * Diagnostic counts: errors, warnings, information.
 */
export type Diag = { e: number, w: number, i: number, };

/**
 * An edit an agent proposes, waiting as a diff (M28).
 */
export type DiffInfo = { 
/**
 * What `accept` and `reject` name.
 */
id: string, 
/**
 * The file it changes (whole path), and relative to the pane's
 * directory when it's inside.
 */
file: string, 
/**
 * Lines added and removed.
 */
added: number, removed: number, 
/**
 * The change as a unified diff, cut short when it's long.
 */
text: string, 
/**
 * It makes a new file.
 */
new?: boolean, at_ms: number, 
/**
 * Which IDE shows it: `arugula`, or the one diffs go to.
 */
ide: string, };

export type Dir = "row" | "column";

/**
 * An agent's write, waiting for a person.
 */
export type Draft = { id: string, 
/**
 * Who drafted it (`mcp:claude-code`, `agent`).
 */
by: string, at_ms: number, status: DraftStatus, 
/**
 * Who sent or dropped it, and when.
 */
settled_by?: string, settled_ms?: number, url?: string, 
/**
 * The last send failed: why (it waits again, text kept).
 */
error?: string, } & ({ "method": "comment", body: string, } | { "method": "review", event: ReviewEvent, body?: string, } | { "method": "merge", style?: string, } | { "method": "rerun_checks" } | { "method": "live", on: boolean, });

export type DraftStatus = "waiting" | "sent" | "dropped";

/**
 * A pane's driver (M13).
 */
export type Driver = { 
/**
 * Principal id (`owner`, `tailnet:<login>`, `account:<id>`).
 */
who: string, name: string, };

/**
 * Where to put something relative to a pane.
 */
export type Edge = "left" | "right" | "top" | "bottom" | "center";

/**
 * What an editor says about itself in summaries (M28, S17's schema):
 * what changes about once in ten seconds. The cursor and the file's text
 * are content and go only to followers ([`ServerMsg::Follow`]).
 */
export type EditorInfo = { 
/**
 * `vscode`, `cursor`, `code-server`, `nvim`, ...
 */
app: string, 
/**
 * VS Code's remote: `ssh-remote`, `dev-container`, ... (`None`: local).
 */
remote?: string | null, 
/**
 * The remote's authority (`ssh-remote+geek`), to open the same file
 * from a desktop editor.
 */
authority?: string | null, 
/**
 * The machine it runs on, as it names itself.
 */
hostname?: string | null, 
/**
 * Diagnostics across the workspace.
 */
diag: Diag, 
/**
 * Files with unsaved changes.
 */
dirty: number, debug?: DebugState | null, 
/**
 * A file with merge conflict markers that's open.
 */
conflict?: string | null, 
/**
 * How many people follow it now: the editor says so.
 */
followers: number, };

export type Event = { 
/**
 * Provider-prefixed, stable across polls (`fj-1403`).
 */
id: string, at: number, actor: string | null, kind: EventKind, 
/**
 * The forge's own name for it, when it's `other` (or more exact).
 */
what?: string, 
/**
 * Who a request is for.
 */
target?: Reviewer, body?: string, 
/**
 * A push: how many commits, and whether it was forced.
 */
commits?: number, force?: boolean, };

export type EventKind = "commented" | "review_comment" | "reviewed" | "review_requested" | "review_request_removed" | "review_dismissed" | "pushed" | "labeled" | "assigned" | "renamed" | "referenced" | "merged" | "closed" | "reopened" | "branch_deleted" | "milestone" | "mentioned" | "other";

/**
 * `GET /api/flags`: one row per flag (the `flags.list` operation).
 */
export type FlagInfo = { name: string, 
/**
 * What Developer settings calls it. Absent from a daemon before #665.
 */
title?: string, about: string, 
/**
 * Whether it is on here now.
 */
on: boolean, default: boolean, 
/**
 * Whether this build has what the flag turns on. A build without Labs
 * can set a flag, and nothing follows; Settings says so. Absent from
 * daemons that don't report it, which have it.
 */
built?: boolean, };

/**
 * `PUT /api/flags/{name}`: the body (the `flag.set` operation).
 */
export type FlagSetRequest = { on: boolean, };

export type FollowChange = { range: [number, number, number, number], text: string, };

export type FollowCursor = { file: string, line: number, col: number, sel?: [number, number, number, number] | null, 
/**
 * The first and last lines in view.
 */
view?: [number, number] | null, 
/**
 * nvim's mode.
 */
mode?: string, };

export type FollowDiagnostic = { range: [number, number, number, number], 
/**
 * `error`, `warning`, `information`, `hint`.
 */
severity: string, message: string, };

export type FollowDiagnostics = { file: string, items: Array<FollowDiagnostic>, };

export type FollowEdit = { file: string, version: number, changes: Array<FollowChange>, };

export type FollowMsg = FollowCursor | { open: FollowOpen, } | { edit: FollowEdit, } | { diagnostics: FollowDiagnostics, } | { gone: true, };

export type FollowOpen = { file: string, version: number, text: string | null, lang?: string, too_big?: boolean, };

/**
 * M40: how the block hears of changes.
 */
export type ForgeLive = "webhook" | "polling";

/**
 * A `tea` login to pick from.
 */
export type ForgeLogin = { name: string, url: string, user: string, };

/**
 * A forge block's state: the PR or issue, what it wants of you, the agent's
 * drafts, and how it's kept current.
 */
export type ForgeState = { provider: Provider, 
/**
 * M37: `pr` or `issue`.
 */
kind: ItemKind, repo: string, number: number, api: string | null, login: string | null, host: string | null, dir: string | null, loading: boolean, error: string | null, 
/**
 * It can only read, and why (GitLab with no glab login: anonymous).
 */
read_only: string | null, 
/**
 * Logins to pick from, when none (or several) matched.
 */
logins: Array<ForgeLogin>, 
/**
 * "You", on the forge.
 */
me: string | null, pr: Pr | null, 
/**
 * M37: the issue, for `kind: issue`.
 */
issue: Issue | null, 
/**
 * M37: the agent on it.
 */
link: AgentLink | null, 
/**
 * M37: a new issue, before (and after) it went out.
 */
new: NewIssue | null, wants: Array<ForgeWant>, 
/**
 * Whether *Rerun checks* is possible here, and where the failed run
 * is when it isn't (Forgejo).
 */
rerun: Rerun | null, drafts: Array<Draft>, updated_ms: number, polls: number, 
/**
 * M40: pokes heard (webhooks through control or straight here).
 */
pokes: number, reads: number, 
/**
 * A client draws it.
 */
watching: boolean, 
/**
 * The last write's result (sent directly), for the client.
 */
said: string | null, 
/**
 * The forge's rate limit and any backing off (M38: GitHub).
 */
rate: RateLimit | null, 
/**
 * M40: webhook (pokes say when to read) or polling, and why.
 */
live: ForgeLive, 
/**
 * Where the pokes come from, when there's a way for them to
 * (`github-app`, `hook`).
 */
live_via: string | null, live_why: string | null, 
/**
 * When it last heard from the webhook path.
 */
live_heard_ms: number | null, 
/**
 * A webhook this daemon made is on the repository.
 */
hook: boolean, };

/**
 * What it wants of you, as the client draws it.
 */
export type ForgeWant = { kind: ForgeWantKind, why: string, };

/**
 * What a PR or issue wants of you, as the client draws it: its kind.
 */
export type ForgeWantKind = "review" | "failed" | "changes" | "mention" | "done" | "assigned";

/**
 * A machine's Fountain runner, for its line in the machine panel and the
 * swarm (M45b). Read in the background, never on the request.
 */
export type FountainRunnerInfo = { 
/**
 * Its name on Fountain (the unit's `--name`).
 */
name: string, 
/**
 * What Fountain says; `None` until read.
 */
online?: boolean, 
/**
 * The runner's `fountain` version, as Fountain has it.
 */
version?: string, 
/**
 * `systemctl is-active fountain-runner`.
 */
unit_active?: boolean, 
/**
 * How many sandboxes it holds (when a runner view counted them).
 */
sandboxes?: number, checked_ms?: number, 
/**
 * What wants the owner (the runner view's attention), if anything.
 */
problem?: string, };

/**
 * A gate that waits for someone (M34): an op stopped before a step until
 * a person approves it. The `gate` reason, its bundle on the swarm's rail,
 * the card and the phone's sheet are all made from this, whichever reader
 * found it (`source`).
 */
export type Gate = { 
/**
 * The member (of a workspace) whose op waits.
 */
member: string, op: string, gate: string, env?: string, 
/**
 * When it started waiting, and when it stops (RFC 3339).
 */
since?: string, expires?: string, 
/**
 * Approvals so far, of how many it needs.
 */
approvals: number, needed: number, 
/**
 * The source's own command for approving it, to show.
 */
command?: string, source: GateSource, 
/**
 * A chant gate: the decisions it enforces, its plan and the member's
 * last release (#617).
 */
why?: GateWhy, };

/**
 * A release, from `status`.
 */
export type GateRelease = { component: string, 
/**
 * When (RFC 3339), who and from which commit.
 */
at: string, actor: string | null, git_sha: string | null, };

/**
 * Where a gate was read, which is how it's approved.
 */
export type GateSource = { "kind": "chant", 
/**
 * The workspace's root, and the member's directory, on that host.
 */
root: string, dir: string, 
/**
 * The machine (sprite) it's on; none for this host.
 */
machine?: string, } | { "kind": "hud", 
/**
 * The box's origin, and the app's name in studio.
 */
box_url: string, app: string, } | { "kind": "forge", 
/**
 * The forge's API base (`https://git.example/api/v1`) and its web
 * address for the PR.
 */
api: string, url: string, number: number, } | { "kind": "point", root: string, machine?: string, 
/**
 * The answer record's id, which `points answer` names.
 */
id: string, 
/**
 * The point's name.
 */
point: string, 
/**
 * What it asks: the point's title (or an ad-hoc question's own
 * text) and what it's about.
 */
question: string, 
/**
 * The answers it takes; empty when the points read didn't say.
 */
choices: Array<PointChoice>, 
/**
 * The answer a model proposed (or leaned to, below its
 * threshold), as one of `choices`' values.
 */
proposed?: string, };

/**
 * What someone about to approve a gate wants to know (#617): the
 * decisions it enforces, the plan it binds and the member's last
 * release.
 */
export type GateWhy = { 
/**
 * The decisions covering the gate's member, most relevant first. None
 * until chant has been asked.
 */
decisions: Array<DecisionRef> | null, 
/**
 * Why they couldn't be read.
 */
note: string | null, 
/**
 * The plan the gate was reached for (`status`'s `planDigest`).
 */
plan_digest: string | null, 
/**
 * The member's latest release in the env watched.
 */
last_release: GateRelease | null, };

/**
 * An ssh invite to a pane (M65). `token`, `command` and the pinning lines
 * are only in the answer that made it; the daemon keeps a hash.
 */
export type GuestInvite = { id: number, pane: number, rw: boolean, reusable: boolean, label: string, created_ms: number, expires_ms: number, 
/**
 * A single-use invite someone has logged in with.
 */
used: boolean, 
/**
 * Guests connected with it now.
 */
sessions: number, token?: string, 
/**
 * What the guest pastes: `ssh` with the host key pinned.
 */
command?: string, 
/**
 * The pinned key as a known-hosts line, for an OpenSSH older than 8.5
 * (no `KnownHostsCommand`).
 */
known_hosts?: string, 
/**
 * The host key's SHA256 fingerprint.
 */
fingerprint?: string, host?: string, port?: number, 
/**
 * Through control's ssh jump host (the daemon is behind NAT).
 */
relay: boolean, 
/**
 * The jump host, as `host[:port]` (with `command` only).
 */
jump?: string, };

/**
 * `POST /api/guests` (M65): an invite to one terminal pane for someone
 * with only OpenSSH.
 */
export type GuestInviteRequest = { pane: number, 
/**
 * They may type (one driver per pane still applies).
 */
rw?: boolean | null, 
/**
 * Good for any number of logins until it ends; else the first spends it.
 */
reusable?: boolean | null, 
/**
 * Seconds until it expires [default: an hour; at most a day, or two
 * hours with `rw`].
 */
ttl_secs?: number | null, 
/**
 * What to call them, on their input [default: `guest`].
 */
label?: string | null, 
/**
 * The address to put in the command [default: the daemon's
 * `--guest-ssh-host`, else its hostname].
 */
host?: string | null, 
/**
 * Through control's ssh jump host (`true`), or straight to this
 * machine (`false`) [default: through control when the daemon is
 * joined to one that has a jump host and no address is named].
 */
relay?: boolean | null, };

/**
 * A tool a hand offers (S33).
 */
export type HandTool = { name: string, description: string, 
/**
 * Its arguments, as a JSON Schema object.
 */
schema: unknown, };

/**
 * The optional parts of a machine, as `GET /api/host` reports them.
 */
export type HostFeatures = { 
/**
 * Some Labs flag is on here (see [`crate::flags`]). Absent from older
 * daemons, and pages treat that as off. Daemons from before each
 * feature had a flag (#665) say only this, so a page reads it for
 * every flag where `flags` is missing.
 */
labs: boolean, 
/**
 * The owner has asked for Developer settings here
 * ([`crate::flags::unlocked`]), so the page offers them.
 */
dev?: boolean, 
/**
 * Browser blocks on ports and editor blocks: block sites are on
 * (`--block-listen`).
 */
blocks: boolean, 
/**
 * VM tabs and panes and *Sandboxes…*: a sandbox provider (wisp).
 */
vms: boolean, 
/**
 * A Fountain login here: `FOUNTAIN_API_KEY`, or the CLI's
 * credentials file.
 */
fountain: boolean, 
/**
 * A studio is linked (`arugula studio login`).
 */
studio: boolean, 
/**
 * Threads on panes and sessions: the `chat` flag. Older daemons leave it
 * out, and pages hide threads there.
 */
threads?: boolean, 
/**
 * Huddles on sessions, likewise.
 */
calls?: boolean, 
/**
 * The names of the Labs flags that are on, whatever else each needs
 * (a login, a provider). `None` from a daemon before #665: see `labs`.
 */
flags?: Array<string>, };

/**
 * `GET /api/host`: who this daemon is.
 */
export type HostInfo = { name: string, version: string, 
/**
 * The app↔daemon protocol it speaks ([`crate::PROTOCOL`], #390).
 * Absent from daemons older than the number, which speak
 * [`crate::PROTOCOL_BASELINE`].
 */
protocol?: number, 
/**
 * Where `tailscale serve` puts the app, when tailscaled told us this
 * node's name (#109): `https://NAME.TAILNET.ts.net`.
 */
tailnet_url?: string, 
/**
 * The owner has come in over the tailnet since the daemon started:
 * serve works (#110).
 */
tailnet_seen?: boolean, 
/**
 * The control this daemon joined, if any (#110).
 */
control?: string, 
/**
 * The team it joined as, if one (#110).
 */
team?: string, 
/**
 * This machine is the account's Fountain runner (M45b: it has the
 * `fountain-runner` unit): what was last read of it.
 */
fountain_runner?: FountainRunnerInfo, 
/**
 * What this machine is set up for (#171, #180): the menus offer only
 * these, or say how to turn them on. Absent from older daemons.
 */
features?: HostFeatures, };

export type Intent = { "op": "new_session", name: string | null, from_pane: number | null, } | { "op": "rename_session", session: number, name: string, } | { "op": "close_session", session: number, } | { "op": "new_tab", session: number, from_pane: number | null, cwd?: string, } | { "op": "rename_tab", tab: number, name: string | null, } | { "op": "close_tab", tab: number, } | { "op": "move_tab", tab: number, session: number, index: number, } | { "op": "split", pane: number, edge: Edge, local?: boolean, cwd?: string, } | { "op": "close_pane", pane: number, } | { "op": "move_pane", pane: number, target: number, edge: Edge, } | { "op": "break_pane", pane: number, session: number, index: number | null, } | { "op": "dock_tab", tab: number, target: number, edge: Edge, } | { "op": "resize_split", split: number, weights: Array<number>, } | { "op": "set_option", scope: OptionScope, name: string, value: string | null, };

/**
 * Someone the owner's `@token` named who can't read the thread (#297):
 * theirs to invite.
 */
export type Invitable = { token: string, 
/**
 * `tailnet:<login>` or `account:<id>`.
 */
who: string, name: string, 
/**
 * Another principal taken to be them (a login by their name).
 */
merged?: string, };

/**
 * How an invite's push went: `sent` once a subscription took it,
 * `pending` while control can't reach them yet, else `unreachable`.
 */
export type InviteDelivery = "sent" | "pending" | "unreachable";

/**
 * What an invite granted: the role they hold now, and whether this invite
 * gave it (`false`: they held it already).
 */
export type InviteGrant = { session: number, principal: string, name: string, role: Role, granted: boolean, };

/**
 * `POST /api/invite`: share a session with someone and tell them, and only
 * them (#233; the owner's).
 */
export type InviteRequest = { session: number, 
/**
 * `tailnet:<login>`, `account:<id>`, or a name: someone shared with,
 * or in a checked roster.
 */
who: string, role?: Role | null, note?: string | null, 
/**
 * Where it opens (default: the session's first pane).
 */
pane?: number | null, 
/**
 * With history (default: from now on), for a new grant.
 */
history?: boolean | null, 
/**
 * An editor may also type on this machine's pane for so long (M14).
 */
drive_minutes?: number | null, 
/**
 * For an `account:` no grant or pin vouches for: their root device,
 * whose fingerprint the owner checked with them.
 */
root?: string | null, 
/**
 * From a thread's mention (#297): the thread (`pane-N`, `session-N`)
 * it opens, the one a "from now" share reads from `msg` on (the
 * message that mentioned them), or all of with `whole_thread`. Other
 * threads start at the share, as ever.
 */
thread?: string | null, msg?: number | null, whole_thread?: boolean | null, };

/**
 * What an invite answers.
 */
export type Invited = { invite: string, grant: InviteGrant, pane: number, delivery: InviteDelivery, 
/**
 * Why it isn't `sent`.
 */
reason: string | null, 
/**
 * Whether they may drive (`drive_minutes`), when that was asked.
 */
drive: boolean | null, };

/**
 * An issue, read whole (M37): the item (no branches), its newest events,
 * and the pull requests that refer to it.
 */
export type Issue = { item: Item, events: Array<Event>, linked: Array<Linked>, };

export type Item = { kind: ItemKind, number: number, url: string, title: string, body: string, 
/**
 * The author's login.
 */
author: string, state: ItemState, draft: boolean, labels: Array<string>, assignees: Array<string>, base: Branch, head: Branch, 
/**
 * `refs/pull/N/head`: what `diff` and `checkout` fetch (never the
 * head's branch: AGit PRs have none).
 */
head_ref: string, merge_base: string | null, 
/**
 * `None` while the forge works it out.
 */
mergeable: boolean | null, 
/**
 * The forge says branch protection blocks a merge (GitHub only;
 * Forgejo can't tell, so false).
 */
blocked: boolean, 
/**
 * Review requests still pending (Forgejo's `requested_reviewers` also
 * keeps whoever already reviewed: see `forge::forgejo`).
 */
requested: Array<Reviewer>, comments: number, updated_at: number, merged_at: number | null, merged_by: string | null, };

export type ItemKind = "pr" | "issue";

export type ItemState = "open" | "closed" | "merged";

export type Layout = { panes: Array<[number, Rect]>, splits: Array<SplitRect>, };

/**
 * A work lease, from `status`'s `leases` (#618): who holds a work item.
 */
export type Lease = { 
/**
 * The work item's id.
 */
item: string, holder: string, 
/**
 * `active`, or `expired` (it can be claimed again).
 */
state: string, expires_at: string | null, 
/**
 * The member whose ledger holds it; none for the workspace's own.
 */
member: string | null, 
/**
 * The fencing token, which a run under the lease names.
 */
token: string | null, 
/**
 * The Arugula agent pane holding it, when a run of one names its token
 * (or, running, its agent session is the holder).
 */
pane: number | null, };

/**
 * M37: a pull request that refers to an issue (from the issue's
 * timeline), or one found by its head branch.
 */
export type Linked = { number: number, title: string, state: ItemState, url: string, 
/**
 * Its head branch, when the forge said.
 */
head?: string, };

/**
 * A machine that blocks can run on instead of this host: today a
 * throwaway wisp sprite (a Firecracker microVM) owned by one pane, and
 * deleted when that pane closes.
 */
export type Machine = { id: number, 
/**
 * Who runs it: `wisp`.
 */
provider: string, 
/**
 * The provider's name for it.
 */
sprite: string, 
/**
 * What to call it ("drifting cedar", M7): a display name for the
 * machines we make. The sprite keeps its own name.
 */
name?: string | null, image: string | null, 
/**
 * What it belongs to; the machine goes when that closes.
 */
owner: Owner, state: MachineState, 
/**
 * Someone else's sandbox, borrowed for a shell (M4b): never created or
 * deleted by us; closing its owner only ends our sessions on it.
 */
borrowed?: boolean, 
/**
 * Made for a guest (M14): their principal id, for their quota.
 */
by?: string | null, };

export type MachineState = "starting" | "running" | "gone";

/**
 * Why a member is the way it is, and who works on it (#618): read when
 * its card is opened, kept until the fingerprint moves.
 */
export type MemberWhy = { 
/**
 * The decisions covering it, most relevant first (the gate card's
 * read). None while reading, or when it couldn't be read.
 */
decisions: Array<DecisionRef> | null, 
/**
 * Commits that changed it while no decision constrained it.
 */
undecided: number | null, 
/**
 * Why the decisions couldn't be read.
 */
note: string | null, 
/**
 * Its recent agent runs, newest first: those of the agent sessions the
 * declaration binds to it, and those that made commits in it.
 */
runs: Array<WorkspaceRun>, 
/**
 * Why the runs couldn't be read.
 */
runs_note: string | null, 
/**
 * How long chant's intent read took.
 */
ms: number | null, };

/**
 * A new issue, before and after it reached the forge.
 */
export type NewIssue = { title: string, body: string, 
/**
 * Who asked for it: a person, `mcp:<client>`, or `an agent`.
 */
by: string, 
/**
 * An agent's: a draft, until a person sends it.
 */
agent: boolean, at_ms: number, status: DraftStatus, settled_by?: string, url?: string, 
/**
 * The last try failed: why.
 */
error?: string, 
/**
 * The agent's pane, where *Ask for changes* sends a person's note.
 */
pane?: number, 
/**
 * The last note asking the agent for changes: it's revising until it
 * drafts again (`revised_ms`).
 */
asked?: ChangesAsked, };

export type Node = { "type": "pane", pane: number, } | { "type": "split", id: number, dir: Dir, children: Array<Child>, };

/**
 * What "needs you" notifications someone other than the owner gets (M29):
 * agents in these sessions, or everything they may edit here ("this team's
 * agents" on a team daemon). The owner always is.
 */
export type NotifyPref = { all: boolean, sessions: Array<number>, };

/**
 * `POST /api/notify`: opt in or out of a session's agents, or all of them.
 */
export type NotifyRequest = { 
/**
 * One session; none: everything you may edit here.
 */
session?: number, on: boolean, };

/**
 * `POST /api/conversations/open` (M33): a Claude Code conversation as an
 * agent block.
 */
export type OpenConversationRequest = { 
/**
 * Its id, or a unique prefix.
 */
id: string, 
/**
 * Then `continue` or `fork` it.
 */
then?: string | null, session?: string | null, split?: number | null, from_pane?: number | null, };

/**
 * What opening a conversation answers: its block (`opened`: made now, not
 * there already), and why `then` didn't go through, if it didn't.
 */
export type OpenConversationResponse = { block: number, opened: boolean, conversation: string, error?: string, };

/**
 * `POST /api/blocks`: open a block of any type.
 */
export type OpenRequest = { type: BlockType, 
/**
 * What the type needs to make it (a URL, an agent command).
 */
config?: Record<string, unknown>, 
/**
 * Session name or id, as for `run`.
 */
session?: string | null, 
/**
 * Split this block instead of opening a tab.
 */
split?: number | null, from_pane?: number | null, 
/**
 * Run it on a new throwaway machine of its own.
 */
vm?: boolean | null, 
/**
 * The new machine's image.
 */
image?: string | null, 
/**
 * Run it on this machine [default: the tab's, when splitting in a VM
 * tab; else this host].
 */
host?: number | null, 
/**
 * On this host, even split in a VM tab.
 */
local?: boolean | null, };

/**
 * What opening a block answers.
 */
export type OpenResponse = { block: number, };

/**
 * Where an option lives, as in tmux: the server, a session, a window (tab)
 * or a pane.
 */
export type OptionScope = { "kind": "global" } | { "kind": "session", "id": number } | { "kind": "tab", "id": number } | { "kind": "pane", "id": number };

/**
 * Opaque named strings that clients keep with the layout (tmux's `@user`
 * options: iTerm2's tab grouping and attach guard, `@affinities`). Saved
 * with the layout; an entry goes when what it belongs to does.
 */
export type Options = { global?: { [key in string]: string }, sessions?: Array<[number, { [key in string]: string }]>, tabs?: Array<[number, { [key in string]: string }]>, panes?: Array<[number, { [key in string]: string }]>, };

/**
 * A machine's owner: one pane (M3b), or a tab whose panes share it (M3c).
 * JSON `{"pane": 3}` or `{"tab": 2}`; a bare number (M3b's layout.json) is
 * a pane.
 */
export type Owner = { "pane": number } | { "tab": number };

export type PaneInfo = { id: number, 
/**
 * Identifies this pane's output stream. Offsets are only meaningful
 * within one epoch; a client holding an offset from another epoch (an
 * earlier daemon) must attach with `None`.
 */
epoch: number, 
/**
 * The pane process's working directory, when known.
 */
cwd: string | null, 
/**
 * The foreground command, when it isn't the shell itself.
 */
command: string | null, 
/**
 * Whether a process is running (false while a restored pane waits for
 * Enter).
 */
running: boolean, policy: Policy, 
/**
 * Running now, per the shell integration.
 */
current: CommandInfo | null, 
/**
 * The last command that finished.
 */
last: CommandInfo | null, attention: Attention, 
/**
 * Why it wants you (M24), when it does.
 */
reason?: Reason | null, 
/**
 * Shell integration for shells started in this pane.
 */
integration: boolean, type: BlockType, 
/**
 * The machine it runs on; `None` is this host.
 */
host: number | null, 
/**
 * A question open in a terminal (Claude Code's AskUserQuestion, through
 * its hook), drawn as a card beside it (M6c).
 */
ask?: import("./blocks/ask").Ask | null, 
/**
 * Who answered its last question or approval, and how (M29), until
 * it asks again. For agent blocks too.
 */
answered?: import("./blocks/ask").Answered | null, 
/**
 * An edit Claude Code in this terminal proposes, waiting as a diff
 * (M28: arugulad as its IDE).
 */
diff?: DiffInfo | null, 
/**
 * Claude Code in this terminal is connected to arugulad as its IDE
 * (M28): lines can be mentioned to it from a followed editor.
 */
claude_ide?: boolean, 
/**
 * Claude Code in this terminal waits for a follow-up (its `arugula
 * inbox` hook, M29): one sent now goes straight in.
 */
inbox?: boolean, 
/**
 * Who is driving it (M13): only their typing reaches it, unless it's
 * in pair mode. `None`: nobody yet (the next to type drives).
 */
driver?: Driver | null, 
/**
 * Its driver typed in it in the last few seconds (#118).
 */
typing?: boolean, 
/**
 * Pair mode: every editor types at once.
 */
pair?: boolean, 
/**
 * Never shown to anyone but the owner (M14).
 */
private?: boolean, 
/**
 * Guests trusted to drive this pane, though it runs on the owner's
 * machine (M14): principal id and until when (ms).
 */
trusted?: Array<[string, number]>, 
/**
 * What it's busy with (M23); `None` for blocks other than terminals
 * and agents.
 */
kind?: WorkKind | null, 
/**
 * What a restart resumes (#146): the agent conversation running in
 * it, as "Claude Code conversation <title>", when its policy says to.
 */
resumes?: string | null, 
/**
 * The git repository it works in, if any (M23).
 */
project?: Project | null, 
/**
 * Output rate (M23).
 */
activity?: Activity | null, 
/**
 * The title its program set (OSC 0/2), if any.
 */
title?: string | null, 
/**
 * The file an editor block shows (M27), relative to its folder.
 */
file?: string | null, 
/**
 * What started it, when that wasn't you: an MCP client (M16).
 */
started_by?: StartedBy | null, 
/**
 * An editor's own report (M28): an editor block's, or someone's
 * editor elsewhere (VS Code, Cursor, nvim) that joined the swarm.
 */
editor?: EditorInfo | null, };

export type PaneOp = { "op": "set_policy", policy: Policy, } | { "op": "purge" } | { "op": "set_integration", on: boolean, } | { "op": "attention", state: Attention, } | { "op": "take_control" } | { "op": "request_control" } | { "op": "give_control", to: string, } | { "op": "release_control" } | { "op": "set_pair", on: boolean, } | { "op": "request_trust" } | { "op": "grant_trust", to: string, minutes: number, } | { "op": "revoke_trust", to: string, } | { "op": "set_private", on: boolean, };

/**
 * A pinned file against the tree read (chant's asset `state`).
 */
export type PinState = "pinned" | "drifted" | "missing" | "stale";

/**
 * One answer a decision point's question takes (#621), from `points
 * --open`: what `points answer --answer` is given, how to show it (`yes`
 * and `no` for a yes-or-no point) and what the points file says it means.
 */
export type PointChoice = { value: string, label: string, means: string | null, };

/**
 * What a pane does when the daemon restores it. Its scrollback always comes
 * back; this decides what runs in it.
 */
export type Policy = { "kind": "none" } | { "kind": "shell" } | { "kind": "rerun", confirm: boolean, } | { "kind": "hook", command: string, } | { "kind": "resume" };

/**
 * A pull request, read whole.
 */
export type Pr = { item: Item, reviews: Array<Review>, checks: Array<Check>, rollup: CheckState | null, 
/**
 * The newest events, oldest first.
 */
events: Array<Event>, };

/**
 * Someone looking at the daemon (M13): one per connected client.
 */
export type Presence = { client: number, 
/**
 * Principal id: one person's clients share it.
 */
who: string, name: string, pic?: string, 
/**
 * The tab it shows, and the pane it's focused on.
 */
tab?: number, pane?: number, };

/**
 * The git repository a pane's working directory is in (M23).
 */
export type Project = { 
/**
 * The repository's top directory.
 */
root: string, 
/**
 * Its last path component.
 */
name: string, };

/**
 * Which forge.
 */
export type Provider = "forgejo" | "github" | "gitlab";

/**
 * Output quoted in a thread message: kept as text, so it stays readable
 * after the pane scrolls or closes.
 */
export type Quote = { pane: number, text: string, };

/**
 * GitHub's rate limit as last heard, and any backing off (M38).
 */
export type RateLimit = { limit: number | null, remaining: number | null, used: number | null, 
/**
 * When the window resets (epoch seconds).
 */
reset: number | null, requests: number, not_modified: number, 
/**
 * Why it's polling slowly, if it is.
 */
backoff: string | null, };

/**
 * Why a pane wants you (M24): what happened, not just "needs you", so a
 * client can explain it, bundle it with others and act on it. Every
 * `needs_input` and `done` pane has one.
 */
export type Reason = { kind: ReasonKind, 
/**
 * When it started wanting you.
 */
since_ms: number, 
/**
 * One line: the question, the command that failed, what finished.
 */
headline: string, command?: string, exit?: number, 
/**
 * `done` and `failed`: how long the command ran.
 */
duration_ms?: number, 
/**
 * Reasons with the same key are one card on a "needs you" rail ("11
 * failed on build-03"): `failed:<machine>`, `exited:<machine>`,
 * `ask:<project>:<agent>`. `None` never bundles.
 */
bundle?: string, 
/**
 * `ask`: what is asked, and how to answer it.
 */
ask?: AskRef, 
/**
 * `gate`: the gate that waits (the first, if several do), which
 * `allow` approves (M34).
 */
gate?: Gate, 
/**
 * What [`api::ActRequest`] can do about it here.
 */
actions: Array<Action>, };

export type ReasonKind = "ask" | "input" | "failed" | "exited" | "done" | "paused" | "errors" | "conflict" | "diff" | "gate";

/**
 * An option chosen or turned down.
 */
export type RecordChoice = { option: string, 
/**
 * The option's label, from the record's options.
 */
label: string | null, 
/**
 * Why it was chosen, or turned down.
 */
why: string | null, };

/**
 * One entry of a record's evidence.
 */
export type RecordEvidence = { title: string | null, 
/**
 * A link.
 */
url?: string, 
/**
 * A workspace file pinned by the hash of its bytes, and how it stands
 * now.
 */
path?: string, pin?: PinState, };

export type Rect = { x: number, y: number, cols: number, rows: number, };

/**
 * Where a remote block's pane lives (#17): a host in the home daemon's
 * list, and the pane's id there.
 */
export type RemoteRef = { host: string, pane: number, };

/**
 * What the failed checks' rerun is: through the API (`api`: GitLab, as
 * `rerun_checks`), or on a forge with none (or no login), the run's page.
 */
export type Rerun = { api: boolean, url: string | null, note: string, 
/**
 * GitHub: how many workflow runs it reruns.
 */
runs?: number, 
/**
 * GitLab: the pipeline's id, `null` when no check says. Absent
 * (outer `None`) for the others.
 */
pipeline?: string | null, };

export type Review = { id: string, author: string | null, state: ReviewState, commit: string | null, 
/**
 * Made on an older head.
 */
stale: boolean, at: number | null, body: string | null, url: string | null, };

export type ReviewEvent = "approve" | "request_changes" | "comment";

export type ReviewState = "approved" | "changes_requested" | "commented" | "dismissed" | "pending";

/**
 * Who a review request is for: a person, or a team (`Org/Name`, or just
 * `Name` when the forge doesn't say).
 */
export type Reviewer = { "user": string } | { "team": string };

/**
 * Ordered: an owner can do anything an editor can, and so on.
 */
export type Role = "viewer" | "editor" | "owner";

/**
 * What a rerun would act on: an Actions run (its number in the repo) and
 * job.
 */
export type RunRef = { id: string, job: string | null, };

/**
 * `POST /api/run`: a new terminal pane.
 */
export type RunRequest = { 
/**
 * Run with the pane's shell (`$SHELL -l -c COMMAND`); none for just a
 * shell.
 */
command?: string | null, 
/**
 * Run it on a new throwaway machine owned by the pane.
 */
vm?: boolean | null, 
/**
 * In a new tab whose panes all share a new throwaway machine.
 */
vm_tab?: boolean | null, 
/**
 * The machine's image (the provider's default if none).
 */
image?: string | null, 
/**
 * On a sandbox that already exists (the provider's name for it), over
 * a plain exec with no daemon there ("open shell", M4b). The sandbox
 * isn't ours: closing the pane leaves it be.
 */
sandbox?: string | null, 
/**
 * Session name or id; created if no session has that name. Default: the
 * session of `from_pane`, else the first.
 */
session?: string | null, 
/**
 * Split this pane instead of opening a tab.
 */
split?: number | null, 
/**
 * With `split`: the new pane runs where the split pane does (its tab's
 * machine, or the sandbox it has a shell on) instead of this host.
 */
join?: boolean | null, 
/**
 * Where it starts. On a machine, a directory there.
 */
cwd?: string | null, policy?: Policy | null, 
/**
 * Where the request comes from (`$ARUGULA_PANE`): the default session
 * and working directory.
 */
from_pane?: number | null, };

export type RunResponse = { pane: number, };

export type SdpKind = "offer" | "answer";

/**
 * Control messages from the server.
 */
export type ServerMsg = { "type": "hello", version: string, client: number, state: State, } | { "type": "state", state: State, } | { "type": "size", pane: number, cols: number, rows: number, } | { "type": "resync", pane: number, } | { "type": "error", id: number | null, message: string, } | { "type": "block", block: number, state: unknown, } | { "type": "pong", id: number, } | { "type": "notice", message: string, } | { "type": "control_request", pane: number, who: string, name: string, } | { "type": "trust_request", pane: number, who: string, name: string, } | { "type": "delta", delta: Delta, } | { "type": "follow", pane: number, msg: FollowMsg, } | { "type": "thread", target: ThreadTarget, msg: ThreadMsg, } | { "type": "call_signal", session: number, from: number, signal: CallSignal, cert?: unknown, } | { "type": "hand_call", id: number, tool: string, args: Record<string, unknown>, from: string, };

export type Session = { id: number, name: string, tabs: Array<number>, };

/**
 * A read-only share of one pane. `token`, `path` and `url` are only in the
 * answer that minted it; the daemon keeps a hash.
 */
export type Share = { id: number, pane: number, created_ms: number, expires_ms: number, token?: string, 
/**
 * `/share/<token>`, on this daemon.
 */
path?: string, 
/**
 * The whole link, on this daemon's tailnet name when it has one.
 */
url?: string, };

/**
 * `POST /api/shares`: a read-only link to one terminal pane.
 */
export type ShareRequest = { pane: number, 
/**
 * Seconds until it expires [default: an hour; at most a week].
 */
ttl_secs?: number | null, };

/**
 * A split's area and how long each child is along the split's direction,
 * so a client can turn a divider drag into new weights.
 */
export type SplitRect = { id: number, dir: Dir, rect: Rect, extents: Array<number>, };

/**
 * Who started a pane or block through MCP (M16).
 */
export type StartedBy = { 
/**
 * `mcp:<client>`, as the pane and history show it.
 */
by: string, 
/**
 * The agent block whose token it came with, if any: that block may
 * drive and close it.
 */
block?: number, };

/**
 * Everything a client needs to draw: sessions in order, each tab's tree
 * and the cell rectangles the server computed for it, and pane details.
 */
export type State = { rev: number, sessions: Array<Session>, tabs: Array<TabView>, panes: Array<PaneInfo>, 
/**
 * Machines that blocks run on, other than this host.
 */
machines: Array<Machine>, 
/**
 * Clients' named options (tmux `@` options), per scope.
 */
options: Options, 
/**
 * For someone who isn't the daemon's owner (M12): their role in each
 * session they see, as `[[session, role], ...]` (JSON object keys
 * can't come back as numbers inside a tagged message). Absent for the
 * owner, who owns everything.
 */
roles?: Array<[number, Role]> | null, 
/**
 * Who else is here and where they're looking (M13), within what this
 * client sees.
 */
presence?: Array<Presence>, 
/**
 * The threads (M61) this person may read that have messages, with how
 * many they haven't read.
 */
threads?: Array<ThreadSummary>, 
/**
 * Huddles (M63) on the sessions this person has a role in.
 */
calls?: Array<Call>, };

export type TabView = { id: number, name: string | null, root: Node, cols: number, rows: number, owner: number | null, zoom: number | null, layout: Layout, };

/**
 * `GET` and `POST /api/team-pins`: the teams pinned here, and those whose
 * rosters this machine checked.
 */
export type TeamPins = { pins: { [key in string]: string }, checked: Array<string>, };

/**
 * `POST /api/team-pins`: the teams the owner's browser pinned, and those
 * it left (#233; the owner's). Their rosters are checked against these.
 */
export type TeamPinsRequest = { 
/**
 * Team id to `<founder device>.<founder's root>`, as the owner's
 * browser pinned it.
 */
pins: { [key in string]: string }, 
/**
 * Teams pinned here that the owner's account is no longer in: their
 * members stop being nameable.
 */
drop: Array<string>, };

/**
 * What handing a post to the pane's agent came to: `delivered` (`false`:
 * queued), or `error`.
 */
export type ThreadAgent = { delivered?: boolean, error?: string, };

/**
 * `GET /api/threads/…`: a thread's messages, as the caller may read them.
 */
export type ThreadMessages = { target: ThreadTarget, messages: Array<ThreadMsg>, };

/**
 * One message in a thread (M61).
 */
export type ThreadMsg = { 
/**
 * 1, 2, 3, ... within its thread.
 */
id: number, 
/**
 * When it was posted (ms since the epoch).
 */
at: number, 
/**
 * Who posted it: a principal id (`owner`, `tailnet:…`, `account:…`),
 * or `mcp:…` for an agent.
 */
who: string, name: string, 
/**
 * M74: the poster's picture when they posted, if they have one.
 */
pic?: string, text: string, 
/**
 * Terminal output it quotes.
 */
quote?: Quote, 
/**
 * Principal ids it @mentions.
 */
mentions?: Array<string>, 
/**
 * The `@` tokens (lowercase) that reached someone: each one naming a
 * person in `mentions`, and the agent's when `to_agent`. The page
 * marks only these; an `@word` that reached no one stays plain.
 */
landed?: Array<string>, 
/**
 * It @mentioned the pane's agent, and went to it as a follow-up.
 */
to_agent?: boolean, 
/**
 * An agent posted it (through MCP).
 */
agent?: boolean, };

/**
 * `POST /api/threads/…`: a message, with output it quotes.
 */
export type ThreadPostRequest = { text?: string | null, quote?: Quote | null, };

/**
 * What a post answers. `agent` is set when an `@agent` went to the pane's
 * agent; `invitable` only in the owner's answer (#297), so nobody else's
 * says who exists.
 */
export type ThreadPosted = { message: ThreadMsg, agent: ThreadAgent | null, unreached: Array<Unreached>, invitable?: Array<Invitable>, };

/**
 * `POST /api/threads/…/read`: the caller has read up to that message.
 */
export type ThreadReadRequest = { upto: number, };

/**
 * A thread as one person has it (M61).
 */
export type ThreadSummary = { target: ThreadTarget, 
/**
 * The newest message's id and time.
 */
last: number, at: number, 
/**
 * Messages from others they haven't read.
 */
unread?: number, 
/**
 * One of those mentions them.
 */
mention?: boolean, };

/**
 * What a thread (M61) is about: a pane, or a session.
 */
export type ThreadTarget = { "pane": number } | { "session": number };

/**
 * An `@` in a post that reached no one, for the poster alone.
 */
export type Unreached = { token: string, why: UnreachedWhy, };

/**
 * Why an `@` in a post reached no one.
 */
export type UnreachedWhy = "agent_needs_pane" | "may_not_drive" | "nobody";

/**
 * What a pane is busy with (M23), for drawing and grouping it without
 * attaching: from the foreground process's command line, else the
 * command the shell integration reported.
 */
export type WorkKind = "shell" | "build" | "test" | "agent" | "server" | "logs" | "editor" | "app" | "pr" | "issue" | "fountain";

/**
 * One current record of a kind the declaration names: a decision, a work
 * item, a session.
 */
export type WorkspaceRecord = { 
/**
 * The kind's name (`decision`, `work`).
 */
kind: string, id: string, title: string | null, state: string | null, 
/**
 * A work item: whether nothing blocks it.
 */
ready: boolean | null, blocked_by: Array<string>, 
/**
 * PinState drift and the like.
 */
warnings: Array<string>, valid: boolean, 
/**
 * What a decision answers.
 */
question?: string, 
/**
 * What it constrains, as the record writes it: `member:<name>`,
 * `path:<path>`.
 */
constrains: Array<string>, 
/**
 * The option chosen, and why. None while proposed.
 */
choice?: RecordChoice, 
/**
 * The options turned down, each with why.
 */
rejected: Array<RecordChoice>, 
/**
 * The records this one replaces, and the one that replaced it (chant
 * derives that from the other's link).
 */
supersedes: Array<string>, superseded_by?: string, 
/**
 * Records that fix this one's consequences without replacing it.
 */
remediated_by: Array<string>, 
/**
 * A work item: the decisions it carries out.
 */
implements: Array<string>, 
/**
 * Who decided, and when (a date); who proposed it.
 */
decided_by?: string, decided_on?: string, proposed_by?: string, 
/**
 * What it cites: links, and workspace files pinned by hash.
 */
evidence: Array<RecordEvidence>, 
/**
 * Whether its author's seal verifies: none when nothing here can say
 * (no seal and no signers file).
 */
attested?: boolean, 
/**
 * How far the commit behind it is trusted: `attested`,
 * `attested-unverifiable-here`, `adopted`, `unattested`.
 */
provenance?: string, 
/**
 * The record's file, from the repository root.
 */
path?: string, };

/**
 * An agent run, from `chant workspace runs` (#618). `WorkspaceRun` in
 * TypeScript, which has a `RunRef` already.
 */
export type WorkspaceRun = { 
/**
 * The `Chant-Run` trailer's value.
 */
id: string, 
/**
 * `running` or `ended`.
 */
state: string | null, 
/**
 * The agent session it ran as, and who it worked for.
 */
agent: string | null, by: string | null, started_at: string | null, ended_at: string | null, 
/**
 * How it ended: done, not_done, failed, cancelled.
 */
outcome: string | null, 
/**
 * The work item it worked on.
 */
unit: string | null, 
/**
 * The lease token it worked under.
 */
lease: string | null, 
/**
 * `<kind>/<id>` of each decision it carried out.
 */
decisions: Array<string>, 
/**
 * The Arugula agent pane that ran it (an Arugula run's id names its
 * block: `arugula-<pane>-<ms>`). The client links it while that pane
 * is open.
 */
pane: number | null, };
