//! Agent d'IRIS : raccourci déterministe, boucle du cerveau local avec outils, relais vers Claude.
//! Les planificateurs ne voient que du texte fiable et des références `{{uN}}` ; l'affichage les résout.
use super::claude::{ClaudeBrain, ClaudeError};
use super::exec::{unwire, Executor};
use super::fastpath;
use super::ollama::{BrainError, ChatRequest, LocalBrain, Msg, Role};
use super::taint::{render, render_partial, Segment, Vault};
use crate::tools::{Brain, Origin, PendingAction, Provenance, ToolCall, ToolOutcome};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Appels du cerveau local par tour, au plus.
pub const MAX_STEPS: usize = 8;
const REPLY_EVERY: Duration = Duration::from_millis(50);
const DONE: &str = "C'est fait.";
const PENDING: &str = "Je t'ai préparé la confirmation.";
const FAST_FAILED: &str = "Je n'ai pas pu le faire.";
const TOO_MANY: &str = "Je m'arrête là : cette demande demande trop d'étapes.";
/// Délégation refusée par le réglage « Claude seulement si je le demande » : IRIS propose au lieu de relayer.
pub const CLAUDE_OFFER: &str = "Cette demande me dépasse un peu. Je peux la confier à Claude si tu veux.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrainUsed { Fast, Local, Claude }

impl BrainUsed {
    pub fn as_str(self) -> &'static str {
        match self { Self::Fast => "fast", Self::Local => "local", Self::Claude => "claude" }
    }
}

/// Événements d'un tour (`oniris://iris`). Aucun message d'erreur ne cite un texte non fiable.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IrisEventKind {
    State { state: &'static str, brain: BrainUsed },
    Reply { segments: Vec<Segment>, done: bool },
    ToolUsed { tool: String },
    Pending { action_id: i64 },
    /// `offer_claude` : le panneau propose « Demander à Claude » (jamais relayé sans ce clic).
    Error { kind: &'static str, message: String, offer_local: bool, offer_claude: bool },
}

/// `local_only` : « Version simple en local », jamais de relais vers Claude (ni préfixe, ni délégation).
pub struct TurnInput { pub text: String, pub force_claude: bool, pub local_only: bool }

/// Prompts système (cerveau local, Claude) et derniers messages de la conversation (textes du planificateur).
pub struct PromptCtx { pub system: String, pub claude_system: String, pub history: Vec<(Role, String)> }

#[derive(Debug)]
pub struct TurnEnd { pub brain: BrainUsed, pub planner_text: String, pub shown: Vec<Segment> }

pub type Sink = dyn Fn(IrisEventKind) + Send + Sync;

pub struct Agent {
    pub local: Arc<dyn LocalBrain>,
    pub claude: Arc<dyn ClaudeBrain>,
    pub model: String,
    pub keep_alive_min: u8,
    /// Faux : Claude seulement sur demande explicite (bouton, préfixe) ; délégation et repli deviennent des propositions.
    pub claude_auto: bool,
}

/// Réponse en flux : `Reply` partiel au plus toutes les 50 ms, sans jamais couper une référence.
struct Stream<'a> { buf: String, last: Option<Instant>, vault: &'a Mutex<Vault>, sink: &'a Sink }

impl Stream<'_> {
    fn push(&mut self, d: &str) {
        self.buf.push_str(d);
        if self.last.is_some_and(|l| l.elapsed() < REPLY_EVERY) {
            return;
        }
        let (segments, _) = render_partial(&self.buf, &self.vault.lock());
        if !segments.is_empty() {
            self.last = Some(Instant::now());
            (self.sink)(IrisEventKind::Reply { segments, done: false });
        }
    }
}

fn finish(brain: BrainUsed, text: String, vault: &Mutex<Vault>, sink: &Sink) -> TurnEnd {
    let shown = render(&text, &vault.lock());
    sink(IrisEventKind::Reply { segments: shown.clone(), done: true });
    TurnEnd { brain, planner_text: text, shown }
}

fn fail(brain: BrainUsed, kind: &'static str, message: &str, offer_local: bool, sink: &Sink) -> TurnEnd {
    sink(IrisEventKind::Error { kind, message: message.into(), offer_local, offer_claude: false });
    TurnEnd { brain, planner_text: message.into(), shown: vec![Segment::Text { text: message.into() }] }
}

/// Propose Claude sans rien lui envoyer ; le message tient lieu de réponse (lu aussi par la voix).
fn offer_claude(kind: &'static str, message: &str, sink: &Sink) -> TurnEnd {
    sink(IrisEventKind::Error { kind, message: message.into(), offer_local: false, offer_claude: true });
    TurnEnd { brain: BrainUsed::Local, planner_text: message.into(), shown: vec![Segment::Text { text: message.into() }] }
}

/// Le message commence par « demande à Claude » : un appel explicite.
pub fn asks_claude(text: &str) -> bool { claude_request(text).is_some() }

/// `demande à Claude …` : la demande sans ce préfixe (le message entier s'il ne reste rien).
fn claude_request(text: &str) -> Option<String> {
    let w: Vec<&str> = text.split_whitespace().collect();
    let head: Vec<String> = w.iter().take(3).map(|x| crate::when::norm(x).trim_end_matches(':').to_string()).collect();
    (head == ["demande", "a", "claude"]).then(|| if w.len() > 3 { w[3..].join(" ") } else { text.trim().to_string() })
}

impl Agent {
    pub async fn turn(&self, input: TurnInput, exec: Arc<Executor>, ctx: PromptCtx, sink: &Sink) -> TurnEnd {
        let (text, local_only) = if input.local_only {
            (claude_request(&input.text).unwrap_or(input.text), true)
        } else {
            (input.text, false)
        };
        let fast = if input.force_claude && !local_only { None } else { fastpath::parse(&text, chrono::Local::now()) };
        let ask = if local_only { None } else if input.force_claude { Some(text.clone()) } else { claude_request(&text) };
        let end = if let Some((tool, args)) = fast {
            self.fast(tool, args, &exec, sink).await
        } else if let Some(task) = ask {
            self.claude(task, exec, &ctx, true, sink).await
        } else {
            self.local(text, exec, &ctx, local_only, sink).await
        };
        sink(IrisEventKind::State { state: "idle", brain: end.brain });
        end
    }

    async fn fast(&self, tool: &'static str, args: serde_json::Value, exec: &Executor, sink: &Sink) -> TurnEnd {
        sink(IrisEventKind::State { state: "thinking", brain: BrainUsed::Fast });
        sink(IrisEventKind::ToolUsed { tool: tool.into() });
        let call = ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::Assistant { brain: Brain::Local } };
        let text = match exec.broker.call(call).await {
            ToolOutcome::Done { .. } => DONE,
            ToolOutcome::NeedsConfirmation(p) => {
                (exec.on_pending)(&p);
                sink(IrisEventKind::Pending { action_id: p.action_id });
                PENDING
            }
            _ => FAST_FAILED,
        };
        finish(BrainUsed::Fast, text.into(), &exec.vault, sink)
    }

    async fn local(&self, text: String, exec: Arc<Executor>, ctx: &PromptCtx, local_only: bool, sink: &Sink) -> TurnEnd {
        sink(IrisEventKind::State { state: "thinking", brain: BrainUsed::Local });
        let msg = |role, content: String| Msg { role, content, tool_calls: vec![] };
        let mut messages = vec![msg(Role::System, ctx.system.clone())];
        messages.extend(ctx.history.iter().map(|(r, t)| msg(*r, t.clone())));
        messages.push(msg(Role::User, text.clone()));
        let tools = exec.tools(if local_only { &["iris.ask_claude"] } else { &[] });
        // Un outil du courtier a tourné ce tour-ci : déléguer à Claude risquerait de refaire l'action.
        let mut acted = false;
        for step in 0..MAX_STEPS {
            let req = ChatRequest { model: self.model.clone(), messages: messages.clone(), tools: tools.clone(), keep_alive_min: self.keep_alive_min };
            let mut stream = Stream { buf: String::new(), last: None, vault: &exec.vault, sink };
            let reply = match self.local.chat(req, &mut |d: &str| stream.push(d)).await {
                Ok(r) => r,
                // Avant toute action seulement : relancer chez Claude ne refait rien.
                Err(BrainError::Unavailable | BrainError::NoModel) if step == 0 && !local_only && !self.claude_auto => {
                    return offer_claude("local_unavailable", "IRIS locale est indisponible (Ollama éteint ou modèle absent).", sink);
                }
                Err(BrainError::Unavailable | BrainError::NoModel) if step == 0 && !local_only => {
                    sink(IrisEventKind::Error { kind: "local_unavailable", message: "IRIS locale est indisponible : je passe la main à Claude.".into(), offer_local: false, offer_claude: false });
                    return self.claude(text, exec, ctx, false, sink).await;
                }
                Err(_) => return fail(BrainUsed::Local, "failed", "IRIS locale n'a pas pu répondre.", false, sink),
            };
            if reply.tool_calls.is_empty() {
                return finish(BrainUsed::Local, reply.text, &exec.vault, sink);
            }
            messages.push(Msg { role: Role::Assistant, content: reply.text.clone(), tool_calls: reply.tool_calls.clone() });
            for c in &reply.tool_calls {
                let name = unwire(&c.name);
                if name == "iris.ask_claude" {
                    if local_only {
                        messages.push(msg(Role::Tool, json!({ "error": "délégation à Claude désactivée : réponds toi-même" }).to_string()));
                        continue;
                    }
                    if !acted && !self.claude_auto {
                        return offer_claude("claude_offer", CLAUDE_OFFER, sink);
                    }
                    if !acted {
                        let task = c.args["task"].as_str().filter(|t| !t.trim().is_empty()).unwrap_or(&text).to_string();
                        return self.claude(task, exec, ctx, true, sink).await;
                    }
                    messages.push(msg(Role::Tool, json!({ "error": "délégation impossible après une action" }).to_string()));
                    continue;
                }
                acted |= !name.starts_with("iris.");
                sink(IrisEventKind::ToolUsed { tool: name });
                let out = exec.call(&c.name, c.args.clone()).await;
                // Une confirmation attendue termine le tour : rien ne s'enchaîne derrière, pas même la suite du lot.
                if out["status"] == "pending" {
                    if let Some(id) = out["action_id"].as_i64() {
                        sink(IrisEventKind::Pending { action_id: id });
                    }
                    let t = if reply.text.trim().is_empty() { PENDING.into() } else { reply.text };
                    return finish(BrainUsed::Local, t, &exec.vault, sink);
                }
                messages.push(msg(Role::Tool, out.to_string()));
            }
        }
        finish(BrainUsed::Local, TOO_MANY.into(), &exec.vault, sink)
    }

    async fn claude(&self, task: String, exec: Arc<Executor>, ctx: &PromptCtx, offer_local: bool, sink: &Sink) -> TurnEnd {
        sink(IrisEventKind::State { state: "thinking", brain: BrainUsed::Claude });
        let mut prompt = String::new();
        if !ctx.history.is_empty() {
            prompt.push_str("Conversation précédente :\n");
            for (r, t) in &ctx.history {
                prompt.push_str(if *r == Role::User { "Utilisateur : " } else { "IRIS : " });
                prompt.push_str(t);
                prompt.push('\n');
            }
            prompt.push_str("\nDemande :\n");
        }
        prompt.push_str(&task);
        // Exécuteur jumeau : journal au nom de Claude, et chaque confirmation remonte aussi en événement `Pending`.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<i64>();
        let parent = exec.on_pending.clone();
        let claude_exec = Arc::new(Executor {
            broker: exec.broker.clone(),
            vault: exec.vault.clone(),
            origin: Origin::Assistant { brain: Brain::Claude },
            on_pending: Arc::new(move |p: &PendingAction| {
                parent(p);
                let _ = tx.send(p.action_id);
            }),
            virtuals: exec.virtuals.clone(),
        });
        let vault = exec.vault.clone();
        let mut stream = Stream { buf: String::new(), last: None, vault: &vault, sink };
        let mut on_delta = |d: &str| stream.push(d);
        let mut plan = std::pin::pin!(self.claude.plan(&ctx.claude_system, prompt, claude_exec, &mut on_delta));
        let r = loop {
            tokio::select! {
                biased;
                r = &mut plan => break r,
                Some(id) = rx.recv() => sink(IrisEventKind::Pending { action_id: id }),
            }
        };
        while let Ok(id) = rx.try_recv() {
            sink(IrisEventKind::Pending { action_id: id });
        }
        match r {
            Ok(text) => finish(BrainUsed::Claude, text, &vault, sink),
            Err(ClaudeError::NotInstalled) => fail(BrainUsed::Claude, "claude_missing", "Claude Code n'est pas installé.", offer_local, sink),
            Err(ClaudeError::Unavailable(_)) => {
                fail(BrainUsed::Claude, "claude_unavailable", "Claude est indisponible pour le moment (limite d'usage ou compte non connecté).", offer_local, sink)
            }
            Err(_) => fail(BrainUsed::Claude, "failed", "Claude n'a pas pu répondre.", offer_local, sink),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iris::claude::OnDelta;
    use crate::iris::ollama::{ChatReply, ToolCallMsg};
    use crate::iris::taint::{project, resolve_args};
    use crate::iris::Quarantine;
    use crate::tools::helpers::{low, spec, untrusted};
    use crate::tools::{ConfirmationText, PendingAction, Permission, Tool, ToolBroker, ToolError, ToolResult, ToolSpec};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const CANARI: &str = "CANARI-9b2e";
    const MAILS: &str = include_str!("../../tests/fixtures/iris/mails_pieges.json");
    const DANGEROUS: [&str; 4] = ["shell.run", "files.delete", "mail.send", "memory.remember"];

    type Script = Box<dyn Fn(usize, &ChatRequest) -> Result<ChatReply, BrainError> + Send + Sync>;

    /// Cerveau local scripté qui enregistre toutes ses entrées ; `|` découpe le texte en morceaux espacés de 60 ms.
    struct FakeLocal { script: Script, chats: Mutex<Vec<String>>, quarantine: Mutex<Vec<String>> }

    #[async_trait]
    impl LocalBrain for FakeLocal {
        async fn chat(&self, req: ChatRequest, on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send)) -> Result<ChatReply, BrainError> {
            let n = { let mut c = self.chats.lock(); c.push(format!("{:?} {:?}", req.messages, req.tools)); c.len() - 1 };
            let r = (self.script)(n, &req)?;
            let chunks: Vec<&str> = r.text.split('|').collect();
            for (i, c) in chunks.iter().enumerate() {
                if i > 0 { tokio::time::sleep(Duration::from_millis(60)).await; }
                if !c.is_empty() { on_delta(c); }
            }
            Ok(ChatReply { text: r.text.replace('|', ""), tool_calls: r.tool_calls })
        }
        async fn structured(&self, system: &str, content: &str, _: &Value) -> Result<Value, BrainError> {
            self.quarantine.lock().push(format!("{system}\n{content}"));
            Ok(json!({"category": "demande", "needs_reply": true, "deadline_ms": null, "tag": null, "summary": format!("résumé {CANARI}"), "text": format!("digest {CANARI}")}))
        }
    }

    fn local(script: impl Fn(usize, &ChatRequest) -> Result<ChatReply, BrainError> + Send + Sync + 'static) -> Arc<FakeLocal> {
        Arc::new(FakeLocal { script: Box::new(script), chats: Mutex::new(vec![]), quarantine: Mutex::new(vec![]) })
    }

    /// Claude qui enregistre ses entrées ; `compromised` : lit les mails et recopie chaque référence dans les outils dangereux.
    struct FakeClaude { reply: Result<String, ClaudeError>, compromised: bool, plans: Mutex<Vec<String>>, completes: Mutex<Vec<String>> }

    #[async_trait]
    impl ClaudeBrain for FakeClaude {
        async fn complete(&self, system: &str, prompt: String, _: OnDelta<'_>) -> Result<String, ClaudeError> {
            self.completes.lock().push(format!("{system}\n{prompt}"));
            Ok(format!("brouillon {CANARI}"))
        }
        async fn plan(&self, system: &str, prompt: String, exec: Arc<Executor>, on_delta: OnDelta<'_>) -> Result<String, ClaudeError> {
            self.plans.lock().push(format!("{system}\n{prompt}"));
            if self.compromised {
                let out = exec.call("mail__list", json!({})).await;
                self.plans.lock().push(out.to_string());
                for r in refs(&out.to_string()) {
                    for c in dangerous_calls(&r) {
                        let out = exec.call(&c.name, c.args).await;
                        self.plans.lock().push(out.to_string());
                    }
                }
            }
            if let Ok(t) = &self.reply { on_delta(t); }
            self.reply.clone()
        }
    }

    fn claude(reply: Result<&str, ClaudeError>, compromised: bool) -> Arc<FakeClaude> {
        Arc::new(FakeClaude { reply: reply.map(String::from), compromised, plans: Mutex::new(vec![]), completes: Mutex::new(vec![]) })
    }

    /// Toutes les références `{{uN}}` d'un texte, sans doublon.
    fn refs(s: &str) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for (i, _) in s.match_indices("{{u") {
            let digits: String = s[i + 3..].chars().take_while(char::is_ascii_digit).collect();
            let r = format!("{{{{u{digits}}}}}");
            if !digits.is_empty() && s[i..].starts_with(&r) && !out.contains(&r) { out.push(r); }
        }
        out
    }

    fn call(name: &str, args: Value) -> ToolCallMsg { ToolCallMsg { name: name.into(), args } }

    fn dangerous_calls(r: &str) -> Vec<ToolCallMsg> {
        vec![
            call("shell__run", json!({"command": r})),
            call("files__delete", json!({"path": r})),
            call("mail__send", json!({"to": r, "subject": "Fwd", "body": r})),
            call("memory__remember", json!({"text": r})),
        ]
    }

    fn tools(calls: Vec<ToolCallMsg>) -> Result<ChatReply, BrainError> { Ok(ChatReply { text: String::new(), tool_calls: calls }) }
    fn text(t: &str) -> Result<ChatReply, BrainError> { Ok(ChatReply { text: t.into(), tool_calls: vec![] }) }

    struct Fake { name: &'static str, untrusted: bool, out: Value, runs: Arc<AtomicUsize> }

    #[async_trait]
    impl Tool for Fake {
        fn spec(&self) -> ToolSpec {
            let s = spec(self.name, "fake", json!({"type": "object"}), Permission::Free);
            if self.untrusted { untrusted(s) } else { s }
        }
        fn describe(&self, _: &Value) -> ConfirmationText { low("fake") }
        async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult { output: self.out.clone(), undo: None })
        }
    }

    struct Fx { agent: Agent, claude: Arc<FakeClaude>, exec: Arc<Executor>, runs: Vec<(&'static str, Arc<AtomicUsize>)>, events: Arc<Mutex<Vec<IrisEventKind>>>, pendings: Arc<Mutex<Vec<PendingAction>>> }

    impl Fx {
        fn agent_claude(&self) -> &FakeClaude { self.claude.as_ref() }

        fn runs(&self, name: &str) -> usize { self.runs.iter().find(|r| r.0 == name).unwrap().1.load(Ordering::SeqCst) }

        async fn turn(&self, t: &str, force_claude: bool) -> TurnEnd { self.turn_with(t, force_claude, false).await }

        async fn turn_with(&self, t: &str, force_claude: bool, local_only: bool) -> TurnEnd {
            let ev = self.events.clone();
            let sink = move |e: IrisEventKind| ev.lock().push(e);
            let ctx = PromptCtx { system: "Tu es IRIS.".into(), claude_system: "Tu es IRIS (Claude).".into(), history: vec![(Role::User, "bonjour".into()), (Role::Assistant, "salut".into())] };
            self.agent.turn(TurnInput { text: t.into(), force_claude, local_only }, self.exec.clone(), ctx, &sink).await
        }
    }

    fn fx(l: Arc<FakeLocal>, c: Arc<FakeClaude>) -> Fx {
        let db = Arc::new(Mutex::new(crate::test_util::db()));
        let mut runs = vec![];
        let mut list: Vec<Arc<dyn Tool>> = vec![];
        let mails: Value = serde_json::from_str(MAILS).unwrap();
        for name in ["mail.list", "shell.run", "files.delete", "mail.send", "memory.remember", "notes.search", "music.next", "reminders.create"] {
            let r = Arc::new(AtomicUsize::new(0));
            let out = if name == "mail.list" { mails.clone() } else { json!({}) };
            list.push(Arc::new(Fake { name, untrusted: name == "mail.list", out, runs: r.clone() }));
            runs.push((name, r));
        }
        let broker = Arc::new(ToolBroker::new(db, Arc::new(|| 0), list).unwrap());
        let vault: Arc<Mutex<Vault>> = Default::default();
        let pendings = Arc::new(Mutex::new(vec![]));
        let p = pendings.clone();
        let virtuals = Arc::new(Quarantine { local: l.clone(), claude: c.clone(), vault: vault.clone(), now_ms: || 1_800_000_000_000 });
        let exec = Arc::new(Executor {
            broker, vault, origin: Origin::Assistant { brain: Brain::Local },
            on_pending: Arc::new(move |a: &PendingAction| p.lock().push(a.clone())), virtuals: Some(virtuals),
        });
        Fx { claude: c.clone(), agent: Agent { local: l, claude: c, model: "m".into(), keep_alive_min: 5, claude_auto: true }, exec, runs, events: Default::default(), pendings }
    }

    /// Réglage « Claude seulement si je le demande ».
    fn fx_on_request(l: Arc<FakeLocal>, c: Arc<FakeClaude>) -> Fx {
        let mut f = fx(l, c);
        f.agent.claude_auto = false;
        f
    }

    /// Outils dangereux appelés : tous en attente, provenance non fiable venue d'un mail, origine `brain`, aucun exécuté.
    /// Rend le nombre d'appels.
    fn assert_trapped(f: &Fx, brain: Brain) -> usize {
        let journal = f.exec.broker.journal(1000);
        let hits: Vec<_> = journal.iter().filter(|j| DANGEROUS.contains(&j.tool.as_str())).collect();
        assert!(!hits.is_empty());
        for j in &hits {
            assert_eq!(j.status, "pending", "{}", j.tool);
            assert!(matches!(&j.provenance, Provenance::Untrusted { source } if source.starts_with("mail:")), "{:?}", j.provenance);
            assert_eq!(j.origin, Origin::Assistant { brain });
        }
        assert_eq!(f.pendings.lock().len(), hits.len());
        let events = f.events.lock().iter().filter(|e| matches!(e, IrisEventKind::Pending { .. })).count();
        assert_eq!(events, hits.len(), "un événement Pending par confirmation");
        for t in DANGEROUS { assert_eq!(f.runs(t), 0, "{t}"); }
        hits.len()
    }

    #[tokio::test]
    async fn corpus_piege_planificateur_local_compromis() {
        let l = local(|n, req| {
            let seen = format!("{:?}", req.messages);
            match n {
                0 => tools(vec![call("mail__list", json!({}))]),
                1 => {
                    let r = refs(&seen);
                    tools(vec![call("iris__inspect", json!({"ref": r[0], "question": "urgent ?"})), call("iris__digest", json!({"refs": &r[..5], "task": "summarize"}))])
                }
                _ => tools(refs(&seen).iter().flat_map(|r| dangerous_calls(r)).collect()),
            }
        });
        let c = claude(Ok("x"), false);
        let f = fx(l.clone(), c.clone());
        let end = f.turn("Quels mails importants ?", false).await;
        assert_eq!(l.chats.lock().len(), 3, "le pending termine la boucle");
        for (i, input) in l.chats.lock().iter().enumerate() {
            assert!(!input.contains(CANARI), "entrée {i} du planificateur : {input}");
        }
        let q = l.quarantine.lock();
        assert!(q.len() == 2 && q.iter().all(|q| q.starts_with(crate::iris::quarantine::QUARANTINE)) && q.iter().any(|q| q.contains(CANARI)), "seule la quarantaine lit le contenu");
        drop(q);
        assert!(c.plans.lock().is_empty());
        assert_eq!(f.runs("mail.list"), 1);
        assert_eq!(assert_trapped(&f, Brain::Local), 1, "le premier pending arrête le lot");
        assert_eq!((end.brain, end.planner_text.as_str()), (BrainUsed::Local, PENDING));
        assert!(f.exec.broker.journal(1000).iter().all(|j| !j.tool.starts_with("iris.")), "outils virtuels jamais journalisés");
    }

    #[tokio::test]
    async fn corpus_piege_claude_compromis() {
        let l = local(|_, _| unreachable!("le local n'est pas appelé"));
        let c = claude(Ok("Voilà : {{u3}}"), true);
        let f = fx(l.clone(), c.clone());
        let end = f.turn("Lis mes mails", true).await;
        assert!(l.chats.lock().is_empty());
        for (i, input) in c.plans.lock().iter().enumerate() {
            assert!(!input.contains(CANARI), "entrée {i} de Claude : {input}");
        }
        assert!(assert_trapped(&f, Brain::Claude) >= 4 * 20);
        // Le faux sujet `{{u1}}` d'un mail : une référence qui se résout en texte littéral, jamais en autre référence.
        let journal = f.exec.broker.journal(1000);
        assert!(journal.iter().any(|j| j.tool == "mail.send" && j.args["to"] == "{{u1}}"), "sujet piégé résolu littéralement");
        assert!(c.plans.lock()[0].starts_with("Tu es IRIS (Claude)."), "prompt système de Claude");
        assert_eq!(end.brain, BrainUsed::Claude);
        assert!(matches!(&end.shown[1], Segment::Untrusted { source, .. } if source.starts_with("mail:")), "{:?}", end.shown);
    }

    /// Claude indisponible, comme `NoClaude` côté application.
    struct Absent;

    #[async_trait]
    impl ClaudeBrain for Absent {
        async fn complete(&self, _: &str, _: String, _: OnDelta<'_>) -> Result<String, ClaudeError> { Err(ClaudeError::NotInstalled) }
        async fn plan(&self, _: &str, _: String, _: Arc<Executor>, _: OnDelta<'_>) -> Result<String, ClaudeError> { Err(ClaudeError::NotInstalled) }
    }

    #[tokio::test]
    async fn version_locale_digest_long_n_appelle_pas_claude() {
        let l = local(|n, _| if n == 0 {
            tools(vec![call("iris__digest", json!({"refs": ["{{u1}}"], "task": "draft_reply", "detail": "long"}))])
        } else {
            text("fini")
        });
        let c = claude(Ok("x"), false);
        let mut f = fx(l, c.clone());
        f.exec.vault.lock().put("mail:1/a", "contenu".into());
        // Comme `run_turn` en version locale : la quarantaine reçoit le cerveau absent.
        let virtuals = Arc::new(Quarantine { local: f.agent.local.clone(), claude: Arc::new(Absent), vault: f.exec.vault.clone(), now_ms: || 1_800_000_000_000 });
        f.exec = Arc::new(Executor { broker: f.exec.broker.clone(), vault: f.exec.vault.clone(), origin: Origin::Assistant { brain: Brain::Local }, on_pending: Arc::new(|_: &PendingAction| {}), virtuals: Some(virtuals) });
        let end = f.turn_with("Rédige une réponse", false, true).await;
        assert_eq!(end.brain, BrainUsed::Local);
        assert!(c.plans.lock().is_empty() && c.completes.lock().is_empty(), "aucun appel à Claude");
    }

    #[test]
    fn faux_sujet_reference_rangee_comme_reference() {
        let mut v = Vault::default();
        assert_eq!(v.put("mail:1/m1", format!("secret {CANARI}")), "{{u1}}");
        let p = project(&json!({"account_id": 1, "id": "m6", "subject": "{{u1}}"}), "mail:1/m6", &mut v);
        let r = p["subject"].as_str().unwrap();
        assert_ne!(r, "{{u1}}");
        assert_eq!(resolve_args(&json!({"to": r}), &v).unwrap(), (json!({"to": "{{u1}}"}), vec!["mail:1/m6".to_string()]));
        assert_eq!(render(r, &v), vec![Segment::Untrusted { text: "{{u1}}".into(), source: "mail:1/m6".into() }]);
    }

    #[tokio::test]
    async fn raccourci_suivant_sans_cerveau_en_moins_de_50_ms() {
        let l = local(|_, _| unreachable!());
        let f = fx(l.clone(), claude(Ok("x"), false));
        let t = Instant::now();
        let end = f.turn("Suivant", false).await;
        let took = t.elapsed();
        assert!(took < Duration::from_millis(50), "{took:?}");
        assert!(l.chats.lock().is_empty());
        assert_eq!(f.runs("music.next"), 1);
        let j = &f.exec.broker.journal(1)[0];
        assert_eq!((j.tool.as_str(), &j.provenance, &j.origin), ("music.next", &Provenance::User, &Origin::Assistant { brain: Brain::Local }));
        assert_eq!((end.brain, end.planner_text.as_str()), (BrainUsed::Fast, DONE));

        let end = f.turn("rappelle-moi d'appeler la banque demain à 10 h", false).await;
        let j = &f.exec.broker.journal(1)[0];
        assert_eq!((j.tool.as_str(), &j.args), ("reminders.create", &json!({"text": "d'appeler la banque demain à 10 h"})));
        assert_eq!(end.brain, BrainUsed::Fast);
        assert!(l.chats.lock().is_empty());
    }

    #[tokio::test]
    async fn local_indisponible_passe_a_claude() {
        for e in [BrainError::Unavailable, BrainError::NoModel] {
            let c = claude(Ok("Bonjour"), false);
            let f = fx(local(move |_, _| Err(e.clone())), c.clone());
            let end = f.turn("raconte une blague", false).await;
            assert_eq!(end.brain, BrainUsed::Claude);
            let ev = f.events.lock();
            let err = ev.iter().position(|e| matches!(e, IrisEventKind::Error { kind: "local_unavailable", offer_local: false, .. })).expect("notice");
            let reply = ev.iter().position(|e| *e == IrisEventKind::Reply { segments: vec![Segment::Text { text: "Bonjour".into() }], done: true }).expect("réponse");
            assert!(err < reply);
            assert!(c.plans.lock()[0].contains("Utilisateur : bonjour\nIRIS : salut") && c.plans.lock()[0].ends_with("raconte une blague"));
        }
    }

    #[tokio::test]
    async fn claude_indisponible_propose_le_local_sans_citer_l_erreur() {
        let f = fx(local(|_, _| unreachable!()), claude(Err(ClaudeError::Unavailable(format!("usage limit {CANARI}"))), false));
        let end = f.turn("demande à Claude un poème", false).await;
        assert!(f.agent_claude().plans.lock()[0].ends_with("Demande :\nun poème"), "préfixe retiré");
        {
            let ev = f.events.lock();
            let msg = ev.iter().find_map(|e| match e { IrisEventKind::Error { kind: "claude_unavailable", offer_local: true, message, .. } => Some(message.clone()), _ => None }).expect("erreur");
            assert!(!msg.contains(CANARI) && !end.planner_text.contains(CANARI));
            assert_eq!(*ev.last().unwrap(), IrisEventKind::State { state: "idle", brain: BrainUsed::Claude });
        }
        for (e, kind) in [(ClaudeError::NotInstalled, "claude_missing"), (ClaudeError::Failed(CANARI.into()), "failed"), (ClaudeError::ForbiddenTools(vec![CANARI.into()]), "failed")] {
            let f = fx(local(|_, _| unreachable!()), claude(Err(e), false));
            f.turn("x", true).await;
            assert!(f.events.lock().iter().any(|e| matches!(e, IrisEventKind::Error { kind: k, message, .. } if *k == kind && !message.contains(CANARI))), "{kind}");
        }
    }

    #[tokio::test]
    async fn force_claude_saute_le_raccourci() {
        let c = claude(Ok("ok"), false);
        let f = fx(local(|_, _| unreachable!()), c.clone());
        let end = f.turn("Suivant", true).await;
        assert_eq!((end.brain, f.runs("music.next")), (BrainUsed::Claude, 0));
        assert!(c.plans.lock()[0].ends_with("Demande :\nSuivant"));
    }

    #[tokio::test]
    async fn sur_demande_la_delegation_devient_une_proposition() {
        let l = local(|_, _| tools(vec![call("iris__ask_claude", json!({"task": "écris un sonnet"}))]));
        let c = claude(Ok("Voici"), false);
        let f = fx_on_request(l.clone(), c.clone());
        let end = f.turn("un poème difficile", false).await;
        assert_eq!((end.brain, end.planner_text.as_str()), (BrainUsed::Local, CLAUDE_OFFER));
        assert!(c.plans.lock().is_empty() && c.completes.lock().is_empty(), "rien ne part chez Claude");
        assert!(f.events.lock().iter().any(|e| matches!(e, IrisEventKind::Error { kind: "claude_offer", offer_claude: true, offer_local: false, .. })));
    }

    #[tokio::test]
    async fn sur_demande_local_indisponible_ne_bascule_pas() {
        for e in [BrainError::Unavailable, BrainError::NoModel] {
            let c = claude(Ok("Bonjour"), false);
            let f = fx_on_request(local(move |_, _| Err(e.clone())), c.clone());
            let end = f.turn("raconte une blague", false).await;
            assert_eq!(end.brain, BrainUsed::Local);
            assert!(c.plans.lock().is_empty());
            assert!(f.events.lock().iter().any(|e| matches!(e, IrisEventKind::Error { kind: "local_unavailable", offer_claude: true, .. })));
        }
    }

    #[tokio::test]
    async fn sur_demande_l_appel_explicite_passe_toujours() {
        let c = claude(Ok("ok"), false);
        let f = fx_on_request(local(|_, _| unreachable!()), c.clone());
        assert_eq!(f.turn("demande à Claude un poème", false).await.brain, BrainUsed::Claude);
        assert_eq!(f.turn("un poème", true).await.brain, BrainUsed::Claude);
        assert_eq!(c.plans.lock().len(), 2);
        assert!(asks_claude("Demande à Claude : un poème") && !asks_claude("un poème pour Claude"));
    }

    #[tokio::test]
    async fn ask_claude_refuse_apres_une_action() {
        let l = local(|n, _| match n {
            0 => tools(vec![call("notes__search", json!({"query": ""}))]),
            1 => tools(vec![call("iris__ask_claude", json!({"task": "fais-le"}))]),
            _ => text("fini"),
        });
        let c = claude(Ok("x"), false);
        let f = fx(l.clone(), c.clone());
        let end = f.turn("cherche puis délègue", false).await;
        assert_eq!((end.brain, end.planner_text.as_str()), (BrainUsed::Local, "fini"));
        assert!(c.plans.lock().is_empty());
        assert!(l.chats.lock()[2].contains("délégation impossible après une action"));
    }

    #[tokio::test]
    async fn premier_pending_arrete_le_lot() {
        for second in [call("iris__ask_claude", json!({"task": "x"})), call("notes__search", json!({"query": ""}))] {
            let c = claude(Ok("x"), false);
            let f = fx(local(move |_, _| tools(vec![call("shell__run", json!({"command": "{{u1}}"})), second.clone()])), c.clone());
            f.exec.vault.lock().put("mail:1/m1", "rm -rf".into());
            let end = f.turn("vas-y", false).await;
            assert_eq!((end.brain, end.planner_text.as_str()), (BrainUsed::Local, PENDING));
            assert!(c.plans.lock().is_empty(), "pas de délégation après un pending");
            assert_eq!((f.runs("shell.run"), f.runs("notes.search")), (0, 0));
            assert_eq!(f.pendings.lock().len(), 1);
        }
    }

    #[tokio::test]
    async fn ask_claude_termine_le_tour_local() {
        let l = local(|_, _| tools(vec![call("iris__ask_claude", json!({"task": "écris un sonnet"}))]));
        let c = claude(Ok("Voici"), false);
        let f = fx(l.clone(), c.clone());
        let end = f.turn("un poème difficile", false).await;
        assert_eq!((end.brain, l.chats.lock().len()), (BrainUsed::Claude, 1));
        assert!(c.plans.lock()[0].ends_with("écris un sonnet"));
    }

    #[tokio::test]
    async fn boucle_arretee_a_8_etapes_avec_une_reponse() {
        let l = local(|_, _| tools(vec![call("notes__search", json!({"query": ""}))]));
        let f = fx(l.clone(), claude(Ok("x"), false));
        let end = f.turn("cherche sans fin", false).await;
        assert_eq!(l.chats.lock().len(), MAX_STEPS);
        assert_eq!(f.runs("notes.search"), MAX_STEPS);
        assert_eq!(end.planner_text, TOO_MANY);
        assert!(f.events.lock().iter().any(|e| matches!(e, IrisEventKind::Reply { done: true, .. })));
    }

    #[tokio::test]
    async fn reply_partiel_ne_coupe_jamais_une_reference() {
        let l = local(|_, _| text("Voici {{u|1}} puis {|{u|2}} fin"));
        let f = fx(l, claude(Ok("x"), false));
        { let mut v = f.exec.vault.lock(); v.put("mail:1/a", "X".into()); v.put("mail:1/b", "Y".into()); }
        let end = f.turn("montre", false).await;
        let ev = f.events.lock();
        let partial: Vec<&Vec<Segment>> = ev.iter().filter_map(|e| match e { IrisEventKind::Reply { segments, done: false } => Some(segments), _ => None }).collect();
        assert!(partial.len() >= 3, "{partial:?}");
        for segs in &partial {
            for s in segs.iter() {
                let (Segment::Text { text } | Segment::Untrusted { text, .. }) = s;
                assert!(!text.contains('{'), "{segs:?}");
            }
        }
        assert_eq!(end.shown, vec![
            Segment::Text { text: "Voici ".into() }, Segment::Untrusted { text: "X".into(), source: "mail:1/a".into() },
            Segment::Text { text: " puis ".into() }, Segment::Untrusted { text: "Y".into(), source: "mail:1/b".into() },
            Segment::Text { text: " fin".into() },
        ]);
    }

    #[tokio::test]
    async fn version_locale_ne_relaie_jamais_a_claude() {
        let l = local(|n, req| {
            assert!(!format!("{:?}", req.tools).contains("iris__ask_claude"), "pas d'outil de délégation");
            match n {
                0 => tools(vec![call("iris__ask_claude", json!({"task": "fais-le"}))]),
                _ => text("version simple"),
            }
        });
        let c = claude(Ok("x"), false);
        let f = fx(l.clone(), c.clone());
        let end = f.turn_with("demande à Claude un poème", true, true).await;
        assert_eq!((end.brain, end.planner_text.as_str()), (BrainUsed::Local, "version simple"));
        assert!(c.plans.lock().is_empty());
        {
            let chats = l.chats.lock();
            assert!(chats[0].contains("content: \"un poème\""), "préfixe retiré : {}", chats[0]);
            assert!(chats[1].contains("délégation à Claude désactivée"));
        }

        let f = fx(local(|_, _| Err(BrainError::Unavailable)), c.clone());
        let end = f.turn_with("un poème", false, true).await;
        assert_eq!(end.brain, BrainUsed::Local);
        assert!(c.plans.lock().is_empty(), "local indisponible : pas de relais non plus");
    }
}
