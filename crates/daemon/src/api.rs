//! The HTTP API (see `arugula_proto::api` for the routes and shapes). The
//! `arugula` CLI uses it over the Unix socket; remote agents can use it
//! over the tailnet, where the same access checks as the web client apply.

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use arugula_proto::{
    Driver, EventKind, Frame, FrameKind, PaneId, SessionId,
    api::{
        AgentRules, AgentsInventory, Answered, ConversationList, ConversationRow, ConversationsQuery, Described, Empty,
        HistoryKind, KeysRequest, OpenConversationRequest, OpenConversationResponse, OpenResponse, Process, RunRequest,
        RunResponse, SecretFinding, SendRequest, WaitRequest, WaitResult,
    },
    op::ops::{
        AdapterInstall, AdaptersList, AgentsGet, AgentsRefresh, ClosePane, ConversationOpen, ConversationsList,
        FlagSet, FlagsList, FountainAgentsGet, ListPanes, MachineReset, MachinesList, NotifyGet, NotifySet, PaneAsk,
        PaneAskWithdraw, PaneAttention, PaneDetection, PaneDiffOf, PaneDrivers, PaneFollowUp, PaneInbox, PaneKeys,
        PaneMouse, PanePermit, PaneProcess, PanePrompt, PaneSend, PaneWait, PushKeyGet, PushSubscribe, PushTest,
        RuleForget, RulesForgetAll, RulesList, ShellEnvGet, ShellEnvRefresh, ThreadGet, ThreadPost, ThreadRead,
    },
};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::stream::{self, StreamExt};
use regex::Regex;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{
    history::{self, Filter},
    keys,
    mux::{Api, AskReply, Cmd},
    ops::OpRoutes,
    osc::strip,
    pane::{CaptureFormat, CaptureScope, PaneHandle, Subscriber, ToClient},
    server::App,
    store::{PaneLog, now_ms},
};

type AppState = State<Arc<App>>;

pub fn routes() -> Router<Arc<App>> {
    let r = Router::new()
        .op::<ListPanes>()
        .route("/api/run", post(run))
        .op::<PaneSend>()
        .op::<PanePrompt>()
        .op::<PaneKeys>()
        .op::<PaneMouse>()
        .op::<PaneAttention>()
        .route("/api/attention", get(attention_list))
        .route("/api/attention/act", post(act))
        .op::<PaneAsk>()
        .op::<PaneAskWithdraw>()
        .op::<PanePermit>()
        .route("/api/panes/{id}/hook", post(hook))
        .op::<PaneInbox>()
        .op::<PaneFollowUp>()
        .route("/api/turn", get(turn))
        .op::<ThreadGet>()
        .op::<ThreadPost>()
        .op::<ThreadRead>()
        .op::<ClosePane>()
        .route("/api/panes/{id}/capture", get(capture))
        .op::<PaneProcess>()
        .op::<PaneDetection>()
        .route("/api/panes/{id}/tail", get(tail))
        .route("/api/panes/{id}/export.cast", get(export))
        .op::<PaneDrivers>()
        .op::<PaneDiffOf>()
        .op::<RulesList>()
        .op::<RulesForgetAll>()
        .op::<RuleForget>()
        .op::<PaneWait>()
        .op::<ShellEnvGet>()
        .op::<ShellEnvRefresh>()
        .op::<FlagsList>()
        .op::<FlagSet>()
        .op::<AgentsGet>()
        .op::<AgentsRefresh>()
        .route("/api/editors", get(editors))
        .route("/api/sessions/{id}/secrets", get(secrets))
        .op::<AdaptersList>()
        .op::<AdapterInstall>()
        .op::<ConversationsList>()
        .op::<ConversationOpen>()
        .route("/api/blocks", post(open_block))
        .route("/api/blocks/{id}", get(describe))
        .route("/api/blocks/{id}/call/{method}", post(call))
        .op::<MachinesList>()
        .op::<MachineReset>()
        .route("/api/panes/{id}/share-machine", post(share_machine))
        .route("/api/events", get(events))
        .route("/api/history", get(history_))
        .route("/api/search", get(search))
        .op::<PushKeyGet>()
        .op::<PushSubscribe>()
        .op::<PushTest>()
        .op::<NotifyGet>()
        .op::<NotifySet>()
        .op::<FountainAgentsGet>();
    // Labs' routes, if this build has them, and the editor's.
    let r = crate::labs::routes(r);
    let r = crate::editors::routes(r);
    // M70: a file onto the pane's host, and its path pasted.
    #[cfg(unix)]
    let r = r
        .route(
            "/api/panes/{id}/upload",
            post(crate::upload::upload).layer(axum::extract::DefaultBodyLimit::max(crate::upload::CHUNK_MAX)),
        )
        .route("/api/panes/{id}/paste", post(crate::upload::paste));
    r
}

pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

pub(crate) fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

pub(crate) type Res<T> = Result<T, ApiError>;

pub(crate) async fn pane(app: &App, id: PaneId) -> Res<PaneHandle> {
    app.mux.api(|r| Api::Pane(id, r)).await.flatten().ok_or(ApiError(StatusCode::NOT_FOUND, format!("no pane %{id}")))
}

/// A subscriber of our own, for streaming a pane's output; detaches when
/// dropped (the HTTP client went away).
struct Tap {
    pane: PaneHandle,
    client: u64,
    rx: crate::pane::ClientRx,
    _ctrl: mpsc::UnboundedReceiver<ToClient>,
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.pane.detach(self.client);
    }
}

static NEXT_TAP: AtomicU64 = AtomicU64::new(1 << 62);

fn tap(pane: PaneHandle, from: u64) -> Tap {
    let client = NEXT_TAP.fetch_add(1, Ordering::Relaxed);
    let (data, rx) = crate::pane::client_queue();
    let (ctrl, _ctrl) = mpsc::unbounded_channel();
    pane.attach(
        Subscriber { client, data, ctrl, principal: crate::acl::Principal::Owner, name: None, device: None },
        Some(from),
    );
    Tap { pane, client, rx, _ctrl }
}

impl Tap {
    /// The next chunk of output (skipping sizes; a snapshot means the tap
    /// fell behind, which only costs a gap for these readers).
    async fn next(&mut self) -> Option<(u64, Vec<u8>)> {
        loop {
            match self.rx.recv().await? {
                ToClient::Frame(bytes) => {
                    let f = Frame::decode(&bytes).ok()?;
                    if f.kind == FrameKind::Output {
                        return Some((f.offset, f.data));
                    }
                }
                ToClient::Msg(_) | ToClient::Json(_) => {}
                ToClient::Close => return None,
            }
        }
    }
}

fn read_log(app: &App, id: PaneId, from: u64) -> (u64, Vec<u8>) {
    PaneLog::open(app.mux.store.pane_dir(id)).and_then(|l| l.read_from(from)).unwrap_or((from, vec![]))
}

// ---------------------------------------------------------------- handlers

async fn run(State(app): AppState, Json(req): Json<RunRequest>) -> Res<Json<RunResponse>> {
    if req.command.as_deref().is_some_and(|c| c.trim().is_empty()) {
        return Err(bad("empty command"));
    }
    match app.mux.api(|r| Api::Run(req, r)).await {
        Some(Ok(pane)) => Ok(Json(RunResponse { pane })),
        Some(Err(e)) => Err(bad(e)),
        None => Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    }
}

/// Every pane that wants you, and why (M24): what `arugula attention`
/// lists.
async fn attention_list(
    State(app): AppState,
    who: Option<axum::Extension<crate::acl::Principal>>,
) -> Res<Json<Vec<arugula_proto::api::AttentionItem>>> {
    let who = who.map(|axum::Extension(w)| w).filter(|w| !w.is_owner());
    Ok(Json(app.mux.api(|r| Api::AttentionList(who, r)).await.unwrap_or_default()))
}

/// Do something about one pane's reason or several (M24): allow or deny an
/// approval, answer or skip a question, or dismiss it. Each pane is checked
/// on its own (editor on its session) and answered on its own.
async fn act(
    State(app): AppState,
    who: Option<axum::Extension<crate::acl::Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    headers: HeaderMap,
    Json(req): Json<arugula_proto::api::ActRequest>,
) -> Res<Response> {
    use arugula_proto::api::{ActResponse, ActResult};
    let who = who.map(|axum::Extension(w)| w).unwrap_or(crate::acl::Principal::Owner);
    let panes = req.targets();
    if panes.is_empty() {
        return Err(bad("name a pane (pane) or several (panes)"));
    }
    // An agent's invite (#234): the owner's alone, and not an agent's on
    // the owner's CLI (a courtesy, as for `call`).
    if !who.is_owner() || crate::invite::agent(&headers) {
        for p in &panes {
            if is_invite(&app, *p).await {
                return Err(ApiError(StatusCode::FORBIDDEN, crate::invite::OWNER_ONLY.into()));
            }
        }
    }
    // M79: a task's consent card is this machine's own account's, not a
    // team's other owner's (who is `Owner` here too).
    if !who.is_owner() || foreign(&app, &dev) {
        for p in &panes {
            if is_agents(&app, *p).await {
                return Err(ApiError(StatusCode::FORBIDDEN, OWN_ACCOUNT_ONLY.into()));
            }
        }
    }
    // All or nothing on access: a list with one pane you can't change is
    // refused whole, so a bundle never half-happens for that reason.
    if !who.is_owner() {
        for p in &panes {
            match app.mux.api(|r| Api::RoleOn(who.clone(), *p, r)).await.flatten() {
                Some((role, _)) if role >= arugula_core::Role::Editor => {}
                Some(_) => {
                    return Err(ApiError(
                        StatusCode::FORBIDDEN,
                        format!("you're watching %{p}'s session; you can't answer for it"),
                    ));
                }
                None => return Err(ApiError(StatusCode::NOT_FOUND, format!("no pane %{p}"))),
            }
        }
    }
    let mut results = Vec::new();
    let by = who_is(&app, who).await;
    for pane in panes {
        let r = act_one(&app, pane, &req, by.clone()).await;
        results.push(ActResult { pane, ok: r.is_ok(), error: r.err() });
    }
    let status = if results.iter().any(|r| r.ok) { StatusCode::OK } else { StatusCode::CONFLICT };
    Ok((status, Json(ActResponse { results })).into_response())
}

async fn act_one(
    app: &App,
    pane: PaneId,
    req: &arugula_proto::api::ActRequest,
    by: Option<Driver>,
) -> Result<(), String> {
    use arugula_proto::{Action, AskWhat, Attention};
    match req.action {
        // An editor's debugger (M28).
        Action::Continue => {
            let b = app
                .mux
                .api(|r| Api::Block(pane, r))
                .await
                .flatten()
                .ok_or_else(|| format!("%{pane} isn't an editor"))?;
            return block_call(app, pane, &b, "continue", serde_json::json!({}), by).await.map(|_| ());
        }
        // An edit waiting as a diff (M28).
        Action::Accept | Action::Reject => {
            let by = by.unwrap_or(Driver { who: "owner".into(), name: "owner".into() });
            let accept = req.action == Action::Accept;
            return app
                .mux
                .api(|r| Api::DiffAnswer(pane, req.id.clone(), accept, req.text.clone(), by, r))
                .await
                .unwrap_or_else(|| Err("the daemon is stopping".into()));
        }
        _ => {}
    }
    // A failed command typed again (M11), once its shell is idle.
    if req.action == Action::Rerun {
        let (reason, block) = app
            .mux
            .api(|r| Api::Reason(pane, r))
            .await
            .flatten()
            .ok_or_else(|| format!("%{pane} has nothing to run again (it was dismissed, or ran since)"))?;
        // M39: a forge block's failed checks run again through its forge.
        if block && reason.actions.contains(&Action::Rerun) {
            let b = app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
            return block_call(app, pane, &b, "rerun_checks", serde_json::json!({}), by).await.map(|_| ());
        }
        let command = reason
            .command
            .filter(|_| reason.actions.contains(&Action::Rerun))
            .ok_or_else(|| format!("%{pane} has no command to run again"))?;
        let line = crate::fs::rerun_line(&command).ok_or_else(|| format!("can't type {command:?} again"))?;
        return crate::fs::type_line(app, pane, line, "rerun").await.map_err(|e| e.to_string());
    }
    if req.action == Action::Dismiss {
        return match app.mux.api(|r| Api::Attention(pane, Attention::Idle, None, r)).await {
            Some(true) => Ok(()),
            _ => Err(format!("no pane %{pane}")),
        };
    }
    let (reason, block) = app
        .mux
        .api(|r| Api::Reason(pane, r))
        .await
        .flatten()
        .ok_or_else(|| format!("%{pane} doesn't want anything (it was answered, or dismissed)"))?;
    // A gate: approve it, or turn it down (#310), through the block that
    // read it. A decision point's question (#621) is answered instead.
    if let Some(g) = reason.gate.filter(|_| reason.kind == arugula_proto::ReasonKind::Gate) {
        if matches!(g.source, arugula_proto::GateSource::Point { .. }) {
            let answer = req.content.as_ref().and_then(|c| c.get("answer")).filter(|a| a.is_string() || a.is_boolean());
            let Some(answer) = answer.filter(|_| req.action == Action::Answer) else {
                return Err(format!(
                    "%{pane} asks a decision: answer it (content {{\"answer\": one of its choices}}) or dismiss it"
                ));
            };
            if req.id.as_ref().is_some_and(|id| *id != g.key()) {
                return Err(format!("%{pane} now asks another decision (that one was answered)"));
            }
            let b = app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
            let args = serde_json::json!({ "key": g.key(), "answer": answer });
            return block_call(app, pane, &b, "answer", args, by).await.map(|_| ());
        }
        let expire = req.action == Action::Expire && reason.actions.contains(&Action::Expire);
        if req.action != Action::Allow && !expire {
            let or_expire = if reason.actions.contains(&Action::Expire) { ", expire it" } else { "" };
            return Err(format!("%{pane} waits at a gate: approve it (allow){or_expire} or dismiss it"));
        }
        if req.id.as_ref().is_some_and(|id| *id != g.key()) {
            return Err(format!("%{pane} now waits at another gate (that one was approved)"));
        }
        let b = app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
        // M36: a review asked of you is approved as a review, sent with the
        // owner's forge login and naming who approved.
        if matches!(g.source, arugula_proto::GateSource::Forge { .. }) {
            let args = serde_json::json!({ "event": "approve", "key": g.key() });
            return block_call(app, pane, &b, "review", args, by).await.map(|_| ());
        }
        let args = serde_json::json!({ "key": g.key() });
        let method = if expire { "expire" } else { "approve" };
        return block_call(app, pane, &b, method, args, by).await.map(|_| ());
    }
    let ask = reason.ask.ok_or_else(|| format!("%{pane} isn't asking anything: dismiss it"))?;
    let id = req.id.clone().unwrap_or(ask.id.clone());
    if id != ask.id {
        return Err(format!("%{pane} now asks something else (it was answered)"));
    }
    let (method, args) = match (req.action, ask.what) {
        (Action::Allow, AskWhat::Approve) => (
            "approve",
            serde_json::json!({ "id": id, "option": req.option.as_deref().unwrap_or("once"), "suggestion": req.suggestion }),
        ),
        (Action::Deny, AskWhat::Approve) => (
            "deny",
            serde_json::json!({ "id": id, "reason": req.message.as_deref().unwrap_or(""), "message": req.message }),
        ),
        (Action::Deny, AskWhat::Question) => ("decline", serde_json::json!({ "id": id })),
        (Action::Answer, AskWhat::Question) => {
            let content = req.content.clone().filter(|c| c.is_object()).ok_or("answer needs content")?;
            ("answer", serde_json::json!({ "id": id, "content": content }))
        }
        (Action::Allow, AskWhat::Question) => return Err(format!("%{pane} asks a question: answer it")),
        (Action::Answer, AskWhat::Approve) => return Err(format!("%{pane} asks for approval: allow or deny it")),
        (Action::Expire, _) => return Err(format!("%{pane} isn't waiting at a gate: nothing to expire")),
        (Action::Dismiss | Action::Continue | Action::Accept | Action::Reject | Action::Rerun, _) => {
            unreachable!("handled above")
        }
    };
    if block {
        let b = app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
        block_call(app, pane, &b, method, args, by).await.map(|_| ())
    } else {
        answer_terminal(app, pane, method, args, by).await.map(|_| ()).map_err(|e| e.1)
    }
}

/// `arugula hook`: one of Claude Code's hook events (M29).
async fn hook(State(app): AppState, Path(id): Path<PaneId>, Json(hook): Json<serde_json::Value>) -> Res<Json<Empty>> {
    app.mux.send(Cmd::Api(Api::Hook(id, hook)));
    Ok(Json(Empty {}))
}

/// Whether a block is an agent's invites (#234): the owner's to answer,
/// and it shows only its own cards.
/// What answering a task's card (M79) from another account says.
const OWN_ACCOUNT_ONLY: &str = "only this machine's own account allows tasks for its agents";

/// Whether the request comes over a channel from a device of another account
/// than this machine's (a team's other owner is `Owner` here all the same).
fn foreign(app: &App, dev: &Option<axum::Extension<crate::e2e::Caller>>) -> bool {
    let own = app.control.enrolled().map(|e| e.saved.cert.account.clone());
    dev.as_ref().is_some_and(|d| Some(&d.0.account) != own.as_ref())
}

/// Whether a block is a Team agents block (M77), which holds task cards.
async fn is_agents(app: &App, id: PaneId) -> bool {
    app.mux.api(|r| Api::Block(id, r)).await.flatten().is_some_and(|b| b.kind() == arugula_proto::BlockType::Agents)
}

pub(crate) async fn is_invite(app: &App, id: PaneId) -> bool {
    app.mux.api(|r| Api::Block(id, r)).await.flatten().is_some_and(|b| b.kind() == arugula_proto::BlockType::Invite)
}

pub(crate) fn not_on_invites() -> ApiError {
    ApiError(StatusCode::FORBIDDEN, "an invite block shows only its own cards".into())
}

/// What to call whoever made a request (M13), to attribute what they did.
pub(crate) async fn who_is(app: &App, who: crate::acl::Principal) -> Option<Driver> {
    app.mux.api(|r| Api::Who(who, r)).await
}

/// A block's method, on someone's behalf (M29): an approval or answer is
/// recorded as theirs, for its card, the pane's history and the audit log.
async fn block_call(
    app: &App,
    pane: PaneId,
    b: &Arc<dyn crate::block::Block>,
    method: &str,
    mut args: serde_json::Value,
    by: Option<Driver>,
) -> Result<serde_json::Value, String> {
    let waiting = b.waiting();
    // #302: whether Arugula carries someone else's approval to chant
    // (`--relayed-by`), which is the daemon's to say, not the caller's.
    if b.kind() == arugula_proto::BlockType::Workspace
        && let Some(o) = args.as_object_mut()
    {
        o.insert("relayed".into(), by.as_ref().is_some_and(|d| d.who != "owner").into());
    }
    let id = args["id"].as_str().map(str::to_owned);
    // The transcript names whoever isn't its owner (the owner's own
    // answers go unremarked, as before M29). A gate's ledger names whoever
    // approved it, the owner too, by their Arugula name (#75).
    let gate = matches!(b.kind(), arugula_proto::BlockType::Workspace | arugula_proto::BlockType::App);
    // A forge block (M36) names everyone who writes through it, the owner
    // too: its log and its drafts say who sent what.
    let forge = b.kind() == arugula_proto::BlockType::Forge;
    let name = by.as_ref().filter(|d| gate || forge || d.who != "owner").map(|d| d.name.as_str());
    let mut args = args;
    // Asking an agent for changes (a forge block's new issue) is a
    // follow-up from this person, recorded as theirs.
    if forge
        && method == "revise"
        && let Some(o) = args.as_object_mut()
    {
        o.insert("by_who".into(), by.as_ref().map_or("owner", |d| d.who.as_str()).into());
    }
    let out = b.call_by(method, args.clone(), name).await?;
    // A workspace's decision point (#621) is answered, as a gate is approved.
    let point = b.kind() == arugula_proto::BlockType::Workspace && method == "answer";
    if (gate && matches!(method, "approve" | "expire"))
        || point
        || (forge && method == "review" && out.get("gate").is_some())
    {
        // Its card closes saying who, and the audit log says so.
        let how = match method {
            "expire" => "expired".to_owned(),
            "answer" => format!("answered {}", out["label"].as_str().unwrap_or_default()),
            _ => "approved".to_owned(),
        };
        if let (Some(by), Ok(g)) = (by, serde_json::from_value::<arugula_proto::Gate>(out["gate"].clone())) {
            app.mux.send(Cmd::Api(Api::Answered(pane, by, g.key(), how, g.headline())));
        }
        return Ok(out);
    }
    let how = match method {
        "approve" if args["option"].as_str().is_some_and(|o| o.starts_with("always")) => "allowed always",
        "approve" => "allowed",
        "deny" => "denied",
        "answer" => "answered",
        "decline" => "skipped",
        _ => return Ok(out),
    };
    if let (Some(by), Some(w)) = (by, waiting.filter(|w| id.as_ref().is_none_or(|i| *i == w.id))) {
        app.mux.send(Cmd::Api(Api::Answered(pane, by, w.id, how.into(), w.headline)));
    }
    Ok(out)
}

/// `call`'s answer to an answered question: whatever a block method says is
/// JSON of its own, so this one is too.
fn answered(Json(a): Json<Answered>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(a).unwrap_or_default())
}

/// A terminal's question answered by `call %N answer|decline|terminal`, or
/// a permission card by `approve|deny` (M29).
async fn answer_terminal(
    app: &App,
    id: PaneId,
    method: &str,
    args: serde_json::Value,
    by: Option<Driver>,
) -> Res<Json<Answered>> {
    let ask_id = args["id"].as_str().map(str::to_owned);
    let reply = match method {
        "answer" => {
            let content = match args.get("content") {
                Some(c) if c.is_object() => c.clone(),
                _ => {
                    let mut c = args.as_object().cloned().unwrap_or_default();
                    c.remove("id");
                    serde_json::Value::Object(c)
                }
            };
            AskReply::Answer(content)
        }
        "decline" => AskReply::Decline,
        // A permission card (M29): `option` once or always (with one of
        // Claude Code's suggestions, by index: `suggestion`, default 0).
        "approve" => AskReply::Allow {
            always: (args["option"].as_str() == Some("always"))
                .then(|| serde_json::json!(args["suggestion"].as_u64().unwrap_or(0))),
        },
        "deny" => {
            let said = args["message"].as_str().or(args["reason"].as_str()).map(str::trim).filter(|m| !m.is_empty());
            let name = by.as_ref().map_or("someone", |b| b.name.as_str());
            let message = match said {
                Some(m) => format!("{name} said no (through Arugula): {m}"),
                None => format!("{name} said no (through Arugula)."),
            };
            AskReply::Deny { message }
        }
        _ => AskReply::Terminal,
    };
    match app.mux.api(|r| Api::AskReply(id, ask_id, reply, by, r)).await {
        Some(Ok(a)) => Ok(Json(Answered { answered: a.id })),
        Some(Err(e)) if e == crate::invite::OWNER_ONLY => Err(ApiError(StatusCode::FORBIDDEN, e)),
        Some(Err(e)) => Err(bad(e)),
        None => Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    }
}

#[derive(Deserialize)]
struct CaptureQuery {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

async fn capture(State(app): AppState, Path(id): Path<PaneId>, Query(q): Query<CaptureQuery>) -> Res<Response> {
    // Any block has a text rendering; terminals have more.
    if let Some(b) = app.mux.api(|r| Api::Block(id, r)).await.flatten() {
        return Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], b.text()).into_response());
    }
    let p = pane(&app, id).await?;
    let format = match q.format.as_deref().unwrap_or("text") {
        "text" => CaptureFormat::Text,
        "ansi" => CaptureFormat::Ansi,
        "html" => CaptureFormat::Html,
        f => return Err(bad(format!("format {f}: text, ansi or html"))),
    };
    let scope = match q.scope.as_deref().unwrap_or("screen") {
        "screen" => CaptureScope::Screen,
        "scrollback" => CaptureScope::Scrollback,
        "last-command" => CaptureScope::LastCommand,
        s => return Err(bad(format!("scope {s}: screen, scrollback or last-command"))),
    };
    let text = tokio::task::spawn_blocking(move || p.capture(format, scope))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| ApiError(StatusCode::GATEWAY_TIMEOUT, "the pane didn't answer".into()))?;
    let ctype = if format == CaptureFormat::Html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" };
    Ok(([(header::CONTENT_TYPE, ctype)], text).into_response())
}

async fn open_block(
    State(app): AppState,
    who: Option<axum::Extension<crate::acl::Principal>>,
    headers: HeaderMap,
    Json(mut req): Json<arugula_proto::api::OpenRequest>,
) -> Res<Json<OpenResponse>> {
    let who = who.map(|axum::Extension(w)| w);
    // An invite block (#234) is MCP's invite_person's to make, for what it
    // checked: never anyone's from here.
    if req.kind == arugula_proto::BlockType::Invite {
        return Err(ApiError(StatusCode::FORBIDDEN, "invite blocks are made by MCP's invite_person".into()));
    }
    // M44: a worn Fountain agent runs on this host with the owner's
    // secrets: the owner's alone.
    if req.kind == arugula_proto::BlockType::Agent
        && !req.config["as_fountain"].is_null()
        && who.as_ref().is_some_and(|w| !w.is_owner())
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "only the owner can wear a Fountain agent here".into()));
    }
    // A studio box (M35), by its app's name: where it is, from studio.
    if req.kind == arugula_proto::BlockType::App {
        req.config = crate::labs::app_config(&req.config).await.map_err(bad)?;
    }
    // A pull request (M36), by its link, OWNER/REPO#N, or N in a clone.
    if req.kind == arugula_proto::BlockType::Forge {
        // M37: a new issue says who asked for it; under an agent (the CLI's
        // header) it's a draft a person sends.
        if req.config["issue"] == "new" && req.config.is_object() {
            let by = who_is(&app, who.clone().unwrap_or(crate::acl::Principal::Owner)).await;
            req.config["by"] = serde_json::json!(by.map(|d| d.name));
            if crate::invite::agent(&headers) {
                req.config["agent"] = true.into();
            }
        }
        req.config = crate::forges::open_config(&req.config, app.control.state_dir()).await.map_err(bad)?;
    }
    // A pane on another daemon (#17) names a host in our list: that's
    // where clients look it up.
    if req.kind == arugula_proto::BlockType::Remote {
        let at = crate::remote::parse(&req.config).map_err(bad)?;
        let list = app.hosts.list();
        if at.host == list.this {
            return Err(bad(format!("{} is this daemon: its panes go in the layout as they are", at.host)));
        }
        if !list.hosts.iter().any(|h| h.name == at.host) {
            return Err(bad(format!("no host {} in this daemon's list", at.host)));
        }
    }
    match app.mux.api(|r| Api::Open(req, who, r)).await {
        Some(Ok(block)) => Ok(Json(OpenResponse { block })),
        Some(Err(e)) => Err(bad(e)),
        None => Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    }
}

/// Agent blocks by the Claude Code session they have.
async fn blocks_by_session(app: &App) -> HashMap<String, PaneId> {
    let mut out = HashMap::new();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        if p.info.kind != arugula_proto::BlockType::Agent {
            continue;
        }
        if let Some(b) = app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten()
            && let Some(sid) = b.config()["session_id"].as_str()
        {
            out.insert(sid.to_owned(), p.info.id);
        }
    }
    out
}

/// Our terminals' and agent blocks' processes on this host (#81): a
/// Claude Code under one of them runs there.
async fn our_pids(app: &App) -> crate::conversations::Ours {
    let mut ours = crate::conversations::Ours::default();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        let id = p.info.id;
        match p.info.kind {
            arugula_proto::BlockType::Terminal => {
                if let Some(pid) = app.mux.api(|r| Api::Pane(id, r)).await.flatten().and_then(|h| h.pid_now()) {
                    ours.panes.insert(pid, id);
                }
            }
            _ => {
                if let Some(pid) = app.mux.api(|r| Api::Block(id, r)).await.flatten().and_then(|b| b.pid()) {
                    ours.blocks.insert(pid, id);
                }
            }
        }
    }
    ours
}

pub async fn list_conversations(app: &App, q: ConversationsQuery) -> Result<ConversationList, String> {
    let blocks = blocks_by_session(app).await;
    let ours: std::collections::HashSet<PaneId> =
        app.mux.api(Api::Panes).await.unwrap_or_default().into_iter().map(|p| p.info.id).collect();
    let pids = our_pids(app).await;
    let list = tokio::task::spawn_blocking(move || {
        let mut ix = crate::conversations::Index::global().lock().unwrap();
        ix.set_ours(pids);
        ix.scan()
    })
    .await
    .map_err(|e| e.to_string())?;
    let words: Vec<String> = q.q.as_deref().unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    let total = list.len();
    let out: Vec<ConversationRow> = list
        .into_iter()
        .filter(|c| crate::conversations::shown(c, q.all))
        .filter(|c| !q.live || c.live.is_some())
        .filter(|c| q.cwd.as_deref().is_none_or(|d| crate::paths::is_under(&c.cwd, d)))
        .filter(|c| {
            let hay = format!(
                "{} {} {} {}",
                c.title,
                c.first_prompt.as_deref().unwrap_or(""),
                c.last_prompt.as_deref().unwrap_or(""),
                c.cwd
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .take(q.limit.unwrap_or(500))
        .map(|mut c| {
            let block = blocks.get(&c.id).copied();
            c.live = c.live.take().map(|l| l.ours(|p| ours.contains(&p)));
            // The adapter of the block that has it, when its scope didn't
            // say (no systemd scopes).
            if let (Some(b), Some(l)) = (block, c.live.as_mut())
                && l.block.is_none()
                && l.entrypoint == "sdk-ts"
            {
                l.block = Some(b);
                l.pane = None;
                l.place = l.place();
            }
            ConversationRow { conversation: serde_json::to_value(&c).unwrap_or_default(), block }
        })
        .collect();
    Ok(ConversationList { conversations: out, total })
}

pub async fn open_conversation_as(
    app: &App,
    who: Option<crate::acl::Principal>,
    req: OpenConversationRequest,
) -> Result<OpenConversationResponse, ApiError> {
    let id = req.id.clone();
    let c = tokio::task::spawn_blocking(move || crate::conversations::Index::global().lock().unwrap().find(&id))
        .await
        .map_err(|e| bad(e.to_string()))?
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, e))?;
    let existing = blocks_by_session(app).await.get(&c.id).copied();
    let (block, opened) = match existing {
        Some(b) => (b, false),
        None => {
            let source = serde_json::to_value(c.source).unwrap_or_default();
            let config = serde_json::json!({
                "agent": "claude",
                "cwd": c.cwd,
                "session_id": c.id,
                "import": { "path": c.path, "source": source, "title": c.title, "model": c.model },
            });
            let open = arugula_proto::api::OpenRequest {
                kind: arugula_proto::BlockType::Agent,
                config,
                session: req.session,
                split: req.split,
                from_pane: req.from_pane,
                vm: false,
                image: None,
                host: None,
                local: true,
            };
            match app.mux.api(|r| Api::Open(open, who, r)).await {
                Some(Ok(b)) => (b, true),
                Some(Err(e)) => return Err(bad(e)),
                None => return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
            }
        }
    };
    let mut out = OpenConversationResponse { block, opened, conversation: c.id, error: None };
    if let Some(then) = req.then.as_deref() {
        let method = match then {
            "continue" | "fork" => then,
            t => return Err(bad(format!("then: {t}? (continue or fork)"))),
        };
        let b = app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| bad("the block went away"))?;
        if let Err(e) = b.call(method, serde_json::json!({})).await {
            out.error = Some(e);
        }
    }
    Ok(out)
}

/// `describe %N`: where a block is and what it's doing, for any type.
async fn describe(State(app): AppState, Path(id): Path<PaneId>) -> Res<Json<Described>> {
    let info = app
        .mux
        .api(Api::Panes)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.info.id == id)
        .ok_or(ApiError(StatusCode::NOT_FOUND, format!("no block %{id}")))?;
    let state = match app.mux.api(|r| Api::Block(id, r)).await.flatten() {
        Some(b) => b.state(),
        None => {
            let st = pane(&app, id).await?.status();
            serde_json::json!({
                "cwd": st.cwd,
                "busy": st.busy,
                "end": st.end,
                "exited": st.exited,
                "current": st.current,
                "last": st.last,
            })
        }
    };
    Ok(Json(Described { info, state }))
}

/// `call %N METHOD [json]`: a block's own methods. Terminals answer `send`,
/// `keys` and `capture` the same way as their own routes.
async fn call(
    State(app): AppState,
    Path((id, method)): Path<(PaneId, String)>,
    who: Option<axum::Extension<crate::acl::Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Res<Json<serde_json::Value>> {
    let mut args: serde_json::Value = if body.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&body).map_err(|e| bad(e.to_string()))?
    };
    let who = who.map(|axum::Extension(w)| w).unwrap_or(crate::acl::Principal::Owner);
    let owner = who.is_owner();
    let by = match method.as_str() {
        // M11: a file block's `open` is the owner's only. M36: a forge
        // block's writes say who sent them.
        "approve" | "expire" | "deny" | "answer" | "decline" | "send" | "terminal" | "open" | "comment" | "review"
        | "merge" | "rerun_checks" | "revise" | "drop" => who_is(&app, who).await,
        _ => None,
    };
    if let Some(b) = app.mux.api(|r| Api::Block(id, r)).await.flatten() {
        // An agent's invites (#234) are the owner's, and not for an agent
        // on the owner's CLI either (as a forge's drafts, a courtesy).
        if b.kind() == arugula_proto::BlockType::Invite && (!owner || crate::invite::agent(&headers)) {
            return Err(ApiError(StatusCode::FORBIDDEN, crate::invite::OWNER_ONLY.into()));
        }
        // M79: a task's card, and taking a grant back, are this machine's
        // own account's: not an editor's, nor a team's other owner's. So is
        // *Run there*, which reaches the account's other machines as this
        // daemon.
        if b.kind() == arugula_proto::BlockType::Agents
            && matches!(
                method.as_str(),
                "answer" | "decline" | "approve" | "deny" | "terminal" | "revoke" | "run_there"
            )
            && (!owner || foreign(&app, &dev))
        {
            return Err(ApiError(StatusCode::FORBIDDEN, OWN_ACCOUNT_ONLY.into()));
        }
        // The CLI says when an agent runs it (CLAUDECODE, AI_AGENT): a forge
        // block makes its writes drafts then (M36). A courtesy, not a
        // boundary.
        if b.kind() == arugula_proto::BlockType::Forge
            && crate::invite::agent(&headers)
            && let Some(o) = args.as_object_mut()
        {
            o.insert("agent".into(), true.into());
        }
        // A question raised on the block (M35) is answered where it waits.
        // (A studio box's gate is approved by the block: `{key}`.)
        let gate = method == "approve" && ["key", "member"].iter().any(|k| args.get(*k).is_some());
        let answering = !gate && matches!(method.as_str(), "answer" | "decline" | "terminal" | "approve" | "deny");
        if !(answering && app.mux.api(|r| Api::Holds(id, r)).await.unwrap_or(false)) {
            return block_call(&app, id, &b, &method, args, by).await.map(Json).map_err(bad);
        }
        return answer_terminal(&app, id, &method, args, by).await.map(answered);
    }
    let p = pane(&app, id).await?;
    match method.as_str() {
        "send" => {
            let req: SendRequest = serde_json::from_value(args).map_err(|e| bad(e.to_string()))?;
            p.mark_input();
            let mut data = req.text.into_bytes();
            if req.enter {
                data.push(b'\r');
            }
            app.mux.send(Cmd::Input { client: None, pane: id, data });
            Ok(Json(serde_json::json!({})))
        }
        "keys" => {
            let req: KeysRequest = serde_json::from_value(args).map_err(|e| bad(e.to_string()))?;
            let modes = p.status().modes;
            let data: Vec<u8> = req.keys.iter().flat_map(|k| keys::key(k, modes)).collect();
            p.mark_input();
            app.mux.send(Cmd::Input { client: None, pane: id, data });
            Ok(Json(serde_json::json!({})))
        }
        "capture" => {
            let text = tokio::task::spawn_blocking(move || p.capture(CaptureFormat::Text, CaptureScope::Screen))
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            Ok(Json(serde_json::json!({ "text": text })))
        }
        "answer" | "decline" | "terminal" | "approve" | "deny" => {
            answer_terminal(&app, id, &method, args, by).await.map(answered)
        }
        m => Err(bad(crate::block::no_method(arugula_proto::BlockType::Terminal, m))),
    }
}

/// Finds a VM pane's shell by the tag in its environment (a session leader
/// carrying `ARUGULA_EXEC=$1`) and prints: its pid, the foreground
/// process's pid, comm, exe, cwd, and argv separated by \x1f.
const GUEST_PROCESS: &str = r#"
for d in /proc/[0-9]*; do
  p=${d#/proc/}
  tr '\0' '\n' <"$d/environ" 2>/dev/null | grep -qx "ARUGULA_EXEC=$1" || continue
  st=$(sed 's/^.*) //' "$d/stat" 2>/dev/null) || continue
  set -- "$1" $st
  [ "$5" = "$p" ] || continue
  f=$7; [ "$f" -gt 0 ] 2>/dev/null || f=$p
  printf '%s\n%s\n' "$p" "$f"
  cat "/proc/$f/comm"
  readlink "/proc/$f/exe" || echo
  readlink "/proc/$f/cwd" || echo
  tr '\0' '\037' <"/proc/$f/cmdline"; echo
  exit 0
done
exit 1
"#;

async fn share_machine(State(app): AppState, Path(id): Path<PaneId>) -> Res<Json<Empty>> {
    match app.mux.api(|r| Api::ShareMachine(id, r)).await {
        Some(Ok(())) => Ok(Json(Empty {})),
        Some(Err(e)) => Err(ApiError(StatusCode::CONFLICT, e)),
        None => Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    }
}

pub(crate) async fn guest_process(app: &App, pane: PaneId, machine: &arugula_proto::Machine) -> Res<Process> {
    let unavailable = |why: String| ApiError(StatusCode::SERVICE_UNAVAILABLE, why);
    let provider = app.mux.provider.clone().ok_or_else(|| unavailable("VM panes aren't set up".into()))?;
    let tag = crate::mux::exec_tag(&app.mux.daemon_id, pane);
    let argv = ["bash", "-c", GUEST_PROCESS, "arugula-process", &tag];
    let (out, code) =
        provider.run(&machine.sprite, &argv).await.map_err(|e| unavailable(format!("unavailable: {e}")))?;
    let text = String::from_utf8_lossy(&out);
    let lines: Vec<&str> = text.lines().collect();
    if code != Some(0) || lines.len() < 6 {
        return Err(ApiError(StatusCode::CONFLICT, "nothing is running in that pane".into()));
    }
    let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    Ok(Process {
        pid: lines[0].parse().unwrap_or(0),
        foreground: lines[1].parse().unwrap_or(0),
        comm: lines[2].to_owned(),
        exe: some(lines[3]),
        cwd: some(lines[4]),
        argv: lines[5].split('\x1f').filter(|a| !a.is_empty()).map(str::to_owned).collect(),
    })
}

#[derive(Deserialize)]
struct TailQuery {
    /// An offset, or `last-command`. Default: the last 64 KB.
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    follow: Option<u8>,
    /// Stop at this offset (not with `follow`): the TUI's copy mode reads
    /// the history before what it has (M32).
    #[serde(default)]
    until: Option<u64>,
    /// Strip escape sequences.
    #[serde(default)]
    text: Option<u8>,
    /// A pane of another host, from its synced history.
    #[serde(default)]
    host: Option<String>,
}

/// A closed pane's output, from its retired log: no following, and offsets
/// only (no command marks).
fn tail_closed(app: &App, id: PaneId, q: &TailQuery) -> Res<Response> {
    let dir = app
        .mux
        .store
        .pane_dirs()
        .into_iter()
        // Just closed, it may not have been moved to `closed/` yet.
        .find(|(p, _, _)| *p == id)
        .map(|(_, _, d)| d)
        .ok_or(ApiError(StatusCode::NOT_FOUND, format!("no pane %{id}")))?;
    let log = PaneLog::open(dir).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let from = match q.from.as_deref() {
        None => log.end().saturating_sub(64 * 1024),
        Some(n) => n.parse().map_err(|_| bad(format!("pane %{id} is closed: from takes an offset")))?,
    };
    let (_, bytes) = log.read_from(from).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(if q.text == Some(1) { strip(&bytes).into_bytes() } else { bytes }.into_response())
}

/// A block that isn't a terminal: its text, then (following) what it adds.
/// Text that changes in place (a tool call finishing) is printed again from
/// the first line that changed.
fn tail_block(app: Arc<App>, id: PaneId, b: Arc<dyn crate::block::Block>, follow: bool) -> Response {
    let first = b.text();
    if !follow {
        return first.into_response();
    }
    drop(b);
    let live = stream::unfold((app, first.clone()), move |(app, mut seen)| async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let b = app.mux.api(|r| Api::Block(id, r)).await.flatten()?;
            let now = b.text();
            if now == seen {
                continue;
            }
            // From the start of the first line that differs.
            let same = seen.bytes().zip(now.bytes()).take_while(|(a, b)| a == b).count();
            let from = now[..same].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let out = now[from..].to_owned();
            seen = now;
            return Some((Ok::<_, Infallible>(Bytes::from(out)), (app, seen)));
        }
    });
    Body::from_stream(stream::once(async move { Ok::<_, Infallible>(Bytes::from(first)) }).chain(live)).into_response()
}

/// `until=idle` (whatever it's doing, it's not working any more) or
/// `until=needs-input`, for any block. An agent's own state says this as
/// soon as a call returns; others go by the daemon's attention.
pub(crate) async fn wait_attention(app: &App, id: PaneId, needs_input: bool) -> Res<WaitResult> {
    use arugula_proto::Attention;
    use arugula_proto::ask::Ask;
    loop {
        let block = app.mux.api(|r| Api::Block(id, r)).await.flatten().map(|b| b.state());
        let found = block.and_then(|s| {
            let a = serde_json::from_value::<Attention>(s["attention"].clone()).ok()?;
            // #606: an agent that will carry on by itself (its turn held for
            // background work, or tasks it left running) isn't idle yet.
            let carries_on = s["held"] == true || s["background"].as_array().is_some_and(|b| !b.is_empty());
            let a = if carries_on && a != Attention::NeedsInput { Attention::Working } else { a };
            // The question it waits on: the first one not already opened.
            let ask = s["asks"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| a["accepted"] != true)
                .and_then(|a| serde_json::from_value::<Ask>(a.clone()).ok());
            Some((a, ask))
        });
        let (state, ask) = match found {
            Some(f) => f,
            None => {
                let summaries = app.mux.api(Api::Panes).await.unwrap_or_default();
                match summaries.into_iter().find(|p| p.info.id == id) {
                    Some(p) => (p.info.attention, p.info.ask),
                    None => return Err(ApiError(StatusCode::GONE, format!("%{id} closed"))),
                }
            }
        };
        let done = if needs_input { state == Attention::NeedsInput } else { state != Attention::Working };
        if done {
            let ask = ask.filter(|_| state == Attention::NeedsInput);
            return Ok(WaitResult::Attention { state, ask: ask.map(Box::new) });
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn tail(State(app): AppState, Path(id): Path<PaneId>, Query(q): Query<TailQuery>) -> Res<Response> {
    if let Some(host) = q.host.clone() {
        return tail_synced(&app, id, host, &q).await;
    }
    if let Some(b) = app.mux.api(|r| Api::Block(id, r)).await.flatten() {
        return Ok(tail_block(app.clone(), id, b, q.follow == Some(1)));
    }
    let p = match pane(&app, id).await {
        Ok(p) => p,
        // Closed: what it left behind.
        Err(ApiError(StatusCode::NOT_FOUND, _)) => return tail_closed(&app, id, &q),
        Err(e) => return Err(e),
    };
    let status = p.status();
    let from = match q.from.as_deref() {
        None => status.end.saturating_sub(64 * 1024),
        Some("last-command") => status
            .current
            .as_ref()
            .or(status.last.as_ref())
            .map(|c| c.start)
            .ok_or_else(|| bad("no command recorded in that pane (is shell integration on?)"))?,
        Some(n) => n.parse().map_err(|_| bad("from: an offset or last-command"))?,
    };
    // Up to the last command's end, unless following.
    let follow = q.follow == Some(1);
    let text = q.text == Some(1);
    let until = match (q.from.as_deref(), follow) {
        (_, false) if q.until.is_some() => q.until,
        (Some("last-command"), false) => status.current.is_none().then(|| status.last.and_then(|l| l.end)).flatten(),
        _ => None,
    };
    let (start, mut bytes) = read_log(&app, id, from);
    if let Some(end) = until {
        bytes.truncate(end.saturating_sub(start) as usize);
    }
    let resume = start + bytes.len() as u64;
    let first = if text { strip(&bytes).into_bytes() } else { bytes };
    if !follow {
        return Ok(first.into_response());
    }
    let tap = tap(p, resume);
    let live = stream::unfold(tap, move |mut tap| async move {
        let (_, data) = tap.next().await?;
        let out = if text { strip(&data).into_bytes() } else { data };
        Some((Ok::<_, Infallible>(Bytes::from(out)), tap))
    });
    let body = stream::once(async move { Ok::<_, Infallible>(Bytes::from(first)) }).chain(live);
    Ok(Body::from_stream(body).into_response())
}

/// What happened after the last input sent to the pane (so `send` then
/// `wait` never misses a command that finished in between).
pub(crate) async fn wait_for(app: &App, id: PaneId, p: PaneHandle, q: &WaitRequest) -> Res<WaitResult> {
    let mut events = app.mux.events();
    let status = p.status();
    let since = status.input_at;
    match q.until.as_str() {
        "command-end" => {
            if status.current.is_none()
                && let Some(l) = status.last.filter(|l| l.start >= since)
            {
                return Ok(WaitResult::CommandEnd { text: l.text, exit: l.exit, start: l.start, end: l.end });
            }
            loop {
                let Ok(e) = events.recv().await else { continue };
                if e.pane == Some(id) && matches!(e.kind, EventKind::CommandEnd { .. }) {
                    let l = p.status().last.unwrap_or_default();
                    return Ok(WaitResult::CommandEnd { text: l.text, exit: l.exit, start: l.start, end: l.end });
                }
                if e.pane == Some(id) && matches!(e.kind, EventKind::Closed) {
                    return Err(ApiError(StatusCode::GONE, format!("pane %{id} closed")));
                }
            }
        }
        "exit" => {
            if let Some(code) = status.exited {
                return Ok(WaitResult::Exit { code });
            }
            loop {
                let Ok(e) = events.recv().await else { continue };
                if e.pane == Some(id)
                    && let EventKind::Exit { code, .. } = e.kind
                {
                    return Ok(WaitResult::Exit { code });
                }
            }
        }
        "match" => {
            let re = Regex::new(q.re.as_deref().ok_or_else(|| bad("match needs re="))?)
                .map_err(|e| bad(format!("re: {e}")))?;
            let mut tap = tap(p, status.end);
            let (start, bytes) = read_log(app, id, since);
            let mut seen = strip(&bytes);
            let mut base = start;
            loop {
                if let Some(m) = re.find(&seen) {
                    return Ok(WaitResult::Match { text: m.as_str().to_owned(), offset: base + m.start() as u64 });
                }
                // Keep a tail so matches across chunk boundaries are found.
                if seen.len() > 1 << 20 {
                    let cut = seen.len() - (1 << 16);
                    let cut = (cut..seen.len()).find(|i| seen.is_char_boundary(*i)).unwrap_or(cut);
                    base += cut as u64;
                    seen.drain(..cut);
                }
                let Some((_, data)) = tap.next().await else {
                    return Err(ApiError(StatusCode::GONE, format!("pane %{id} closed")));
                };
                seen.push_str(&strip(&data));
            }
        }
        u => Err(bad(format!("until {u}: command-end, exit, match, idle or needs-input"))),
    }
}

/// `wait` for MCP (M16): the same waits, with no limit of their own (the
/// caller has one) and the error as a sentence.
pub(crate) async fn wait_until(app: &App, id: PaneId, until: &str, re: Option<String>) -> Result<WaitResult, String> {
    if until == "idle" || until == "needs-input" {
        return wait_attention(app, id, until == "needs-input").await.map_err(|e| e.1);
    }
    let p = pane(app, id).await.map_err(|e| e.1)?;
    let q = WaitRequest { until: until.to_owned(), re, timeout: None };
    wait_for(app, id, p, &q).await.map_err(|e| e.1)
}

/// Allow, deny, answer or skip what a pane asks, as `by` (MCP's
/// `agent_respond`, M16).
pub(crate) async fn act_as(
    app: &App,
    pane: PaneId,
    req: &arugula_proto::api::ActRequest,
    by: Driver,
) -> Result<(), String> {
    act_one(app, pane, req, Some(by)).await
}

async fn export(State(app): AppState, Path(id): Path<PaneId>) -> Res<Response> {
    let dir = app
        .mux
        .store
        .pane_dirs()
        .into_iter()
        .find(|(p, _, _)| *p == id)
        .map(|(_, _, d)| d)
        .ok_or(ApiError(StatusCode::NOT_FOUND, format!("no history for pane %{id}")))?;
    let cast = tokio::task::spawn_blocking(move || history::export_cast(&dir, &format!("Arugula pane %{id}")))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/x-asciicast")], cast).into_response())
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Comma-separated event types.
    #[serde(rename = "type", default)]
    types: Option<String>,
    #[serde(default)]
    follow: Option<u8>,
    /// Without follow: how far back, in seconds (default an hour).
    #[serde(default)]
    since: Option<u64>,
}

fn event_type(kind: &EventKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v["type"].as_str().map(str::to_owned)).unwrap_or_default()
}

async fn events(State(app): AppState, Query(q): Query<EventsQuery>) -> Res<Response> {
    let types: Option<Vec<String>> = q.types.map(|t| t.split(',').map(|s| s.trim().to_owned()).collect());
    let keep = move |e: &arugula_proto::Event| {
        q.pane.is_none_or(|p| e.pane == Some(p))
            && types.as_ref().is_none_or(|t| t.iter().any(|t| *t == event_type(&e.kind)))
    };
    let line = |e: &arugula_proto::Event| {
        let mut s = serde_json::to_string(e).unwrap_or_default();
        s.push('\n');
        Bytes::from(s)
    };
    if q.follow != Some(1) {
        let since = now_ms().saturating_sub(q.since.unwrap_or(3600) * 1000);
        let store = app.mux.store.clone();
        let mut all: Vec<arugula_proto::Event> = tokio::task::spawn_blocking(move || {
            store
                .pane_dirs()
                .into_iter()
                .filter(|(_, open, _)| *open)
                .flat_map(|(id, _, dir)| history::stored_events(&dir, id, since))
                .collect()
        })
        .await
        .unwrap_or_default();
        all.retain(&keep);
        all.sort_by_key(|e| e.at_ms);
        let body: Vec<u8> = all.iter().flat_map(|e| line(e).to_vec()).collect();
        return Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response());
    }
    let rx = app.mux.events();
    let s = stream::unfold(rx, move |mut rx| {
        let keep = keep.clone();
        async move {
            loop {
                match rx.recv().await {
                    Ok(e) if keep(&e) => return Some((Ok::<_, Infallible>(line(&e)), rx)),
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return None,
                }
            }
        }
    });
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], Body::from_stream(s)).into_response())
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    pane: Option<PaneId>,
    #[serde(default)]
    failed: Option<u8>,
    /// Seconds back.
    #[serde(default)]
    since: Option<u64>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(rename = "match", default)]
    matching: Option<String>,
    /// `command`, `answer` or `agent`; none is all.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// Another host's synced history (`*`: every host's).
    #[serde(default)]
    host: Option<String>,
}

/// Things that look like credentials, for warning before sharing (M14):
/// common token shapes. A heuristic; it says so where it's shown.
fn secret_kinds() -> &'static [(&'static str, Regex)] {
    static KINDS: std::sync::OnceLock<Vec<(&'static str, Regex)>> = std::sync::OnceLock::new();
    KINDS.get_or_init(|| {
        [
            ("a GitHub token", r"\b(gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{40,})"),
            ("an Anthropic key", r"\bsk-ant-[A-Za-z0-9_-]{20,}"),
            ("an API key", r"\bsk-[A-Za-z0-9]{32,}"),
            ("an AWS key", r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"),
            ("a Slack token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
            ("a private key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
            ("a JWT", r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"),
            ("a password", r"(?i)\b(password|passwd|secret)\s*[:=]\s*\S{6,}"),
        ]
        .into_iter()
        .map(|(k, re)| (k, Regex::new(re).expect("secret pattern")))
        .collect()
    })
}

pub fn find_secrets(text: &str) -> Vec<&'static str> {
    secret_kinds().iter().filter(|(_, re)| re.is_match(text)).map(|(k, _)| *k).collect()
}

/// Panes of a session whose recent output looks like it holds a secret.
async fn secrets(State(app): AppState, Path(id): Path<SessionId>) -> Res<Response> {
    let panes = app
        .mux
        .api(|r| Api::SessionEnds(id, r))
        .await
        .flatten()
        .ok_or(ApiError(StatusCode::NOT_FOUND, format!("no session ${id}")))?;
    let mut found: Vec<SecretFinding> = Vec::new();
    for p in panes.keys() {
        let Ok(h) = pane(&app, *p).await else { continue };
        let text = tokio::task::spawn_blocking(move || h.capture(CaptureFormat::Text, CaptureScope::Scrollback))
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        // Recent output: the last 300 lines.
        let recent: Vec<&str> = text.lines().rev().take(300).collect();
        let kinds = find_secrets(&recent.join("\n"));
        if !kinds.is_empty() {
            found.push(SecretFinding { pane: *p, kinds: kinds.into_iter().map(str::to_owned).collect() });
        }
    }
    Ok(Json(found).into_response())
}

async fn history_(State(app): AppState, Query(q): Query<HistoryQuery>) -> Res<Response> {
    let matching = q.matching.as_deref().map(Regex::new).transpose().map_err(|e| bad(format!("match: {e}")))?;
    let kind = q
        .kind
        .as_deref()
        .map(|k| {
            HistoryKind::parse(k).ok_or_else(|| bad(format!("kind: {k:?} isn't command, answer, agent or action")))
        })
        .transpose()?;
    let filter = Filter {
        pane: q.pane,
        failed: q.failed == Some(1),
        kind,
        since_ms: q.since.map(|s| now_ms().saturating_sub(s * 1000)),
        cwd: q.cwd,
        matching,
    };
    let store = app.mux.store.clone();
    let limit = q.limit.unwrap_or(100);
    let synced = app.synced.clone();
    let list = tokio::task::spawn_blocking(move || match &q.host {
        Some(host) => synced.history(host, &filter, limit),
        None => history::history(&store, &filter, limit),
    })
    .await
    .unwrap_or_default();
    Ok(Json(list).into_response())
}

#[derive(Deserialize)]
struct SearchQuery {
    re: String,
    #[serde(default)]
    since: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
    /// Another host's synced history (`*`: every host's).
    #[serde(default)]
    host: Option<String>,
}

async fn search(State(app): AppState, Query(q): Query<SearchQuery>) -> Res<Response> {
    let re = Regex::new(&q.re).map_err(|e| bad(format!("re: {e}")))?;
    let since = q.since.map(|s| now_ms().saturating_sub(s * 1000));
    let store = app.mux.store.clone();
    let kinds = app.mux.kinds.clone();
    let limit = q.limit.unwrap_or(100);
    let synced = app.synced.clone();
    let hits = tokio::task::spawn_blocking(move || match &q.host {
        Some(host) => synced.search(host, &re, since, limit),
        None => history::search(&store, &kinds, &re, since, limit),
    })
    .await
    .unwrap_or_default();
    Ok(Json(hits).into_response())
}

/// A pane synced from another host: its output from an offset (default the
/// last 64 KB), no following.
async fn tail_synced(app: &App, id: PaneId, host: String, q: &TailQuery) -> Res<Response> {
    let synced = app.synced.clone();
    let from = match q.from.as_deref() {
        None => None,
        Some(n) => Some(n.parse::<u64>().map_err(|_| bad("a synced pane's from takes an offset"))?),
    };
    let read = tokio::task::spawn_blocking(move || {
        let from = from.unwrap_or_else(|| synced.pane(&host, id).log_end.saturating_sub(64 * 1024));
        synced.read_from(&host, id, from)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let (_, bytes) = read.map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(if q.text == Some(1) { strip(&bytes).into_bytes() } else { bytes }.into_response())
}

/// A machine's shell is found by its tag.
#[cfg(all(test, target_os = "linux"))]
mod guest_process_tests {
    #[test]
    fn a_shell_is_found_by_its_tag() {
        let tag = format!("t534-{}", std::process::id());
        let mut leader =
            std::process::Command::new("setsid").args(["sleep", "30"]).env("ARUGULA_EXEC", &tag).spawn().unwrap();
        let pid = leader.id().to_string();
        let found = (0..50).find_map(|_| {
            let out =
                std::process::Command::new("bash").args(["-c", super::GUEST_PROCESS, "p", &tag]).output().unwrap();
            let first = String::from_utf8_lossy(&out.stdout).lines().next().map(str::to_owned);
            (first.as_deref() == Some(pid.as_str())).then_some(()).or_else(|| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                None
            })
        });
        let _ = leader.kill();
        let _ = leader.wait();
        assert!(found.is_some());
    }
}

/// Home and the environment an agent block gets.
pub(crate) async fn agent_env(app: &App) -> Res<(std::path::PathBuf, Vec<(String, String)>)> {
    let (home, env) = app
        .mux
        .api(Api::AgentEnv)
        .await
        .ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into()))?;
    // #161: as an agent block gets it, with the user's shell environment.
    let shell = app.mux.shell_env.local().await;
    Ok((home, crate::shellenv::merge(&env, &shell, None)))
}

pub(crate) fn agents_json(snap: &crate::inventory::Snapshot) -> AgentsInventory {
    let (run, off): (Vec<_>, Vec<_>) = arugula_vt::detect::AGENTS.iter().map(|a| a.id).partition(|id| snap.runs(id));
    let own = |ids: Vec<&str>| ids.into_iter().map(str::to_owned).collect();
    AgentsInventory {
        inventory: serde_json::to_value(snap).unwrap_or_default(),
        rules: AgentRules { run: own(run), off: own(off) },
    }
}

/// `GET /api/editors` (M28): every editor in the swarm, joined or a block.
async fn editors(State(app): AppState, who: Option<axum::Extension<crate::acl::Principal>>) -> Json<serde_json::Value> {
    let who = who.map(|axum::Extension(w)| w).filter(|w| !w.is_owner());
    Json(app.mux.api(|r| Api::Editors(who, r)).await.unwrap_or_default().into())
}

/// ICE servers for a huddle (M63): TURN credentials from control, or STUN.
async fn turn(State(app): AppState) -> Json<arugula_control_wire::IceServers> {
    Json(app.control.ice_servers().await)
}

#[cfg(test)]
mod secret_tests {
    #[test]
    fn token_shapes() {
        assert_eq!(super::find_secrets("export GH=ghp_0123456789abcdefghijABCDEFGHIJ012345"), ["a GitHub token"]); // gitleaks:allow (a made-up token)
        assert_eq!(super::find_secrets("key: sk-ant-api03-abcdefghijklmnopqrstuv"), ["an Anthropic key"]);
        assert!(super::find_secrets("AKIAIOSFODNN7EXAMPLE").contains(&"an AWS key"));
        assert!(super::find_secrets("-----BEGIN OPENSSH PRIVATE KEY-----").contains(&"a private key"));
        assert!(super::find_secrets("PASSWORD=hunter22").contains(&"a password"));
        assert!(super::find_secrets("cargo build --release\n   Compiling foo").is_empty());
    }
}
