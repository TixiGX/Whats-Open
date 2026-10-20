//! Interfaccia: lista chat, conversazione, lettori media, impostazioni.

pub mod chat;
pub mod gfx;
pub mod login;
pub mod overlays;
pub mod sidebar;
pub mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, RichText, TextureHandle};

use crate::config::Config;
use crate::engine::{Cmd, Engine, Event, ReplyTarget};
use crate::model::{Chat, Kind, Message, Status};
use crate::play::audio::AudioPlayer;
use crate::play::record::Recorder;
use crate::play::video::VideoPlayer;
use crate::model::Contact;
use crate::store::Store;
use crate::theme::Palette;
use crate::util::{self, Paths};

use gfx::Textures;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Conn {
    Starting,
    Qr,
    PairCode,
    Connected,
    Error,
}

pub enum Pending {
    Image { chat: String, id: String, open: bool, view_once: bool },
    Video { chat: String, id: String, view_once: bool },
    Audio { chat: String, id: String, autoplay: bool },
    Thumb { chat: String, id: String },
    Doc { chat: String, id: String },
}

pub enum Action {
    OpenImage(usize),
    OpenVideo(usize),
    PlayAudio(usize),
    OpenDoc(usize),
    Download(usize),
    Reply(usize),
    React(usize, String),
    ClearReaction(usize),
    CopyText(usize),
    Revoke(usize),
    /// Svela un contenuto "una volta sola" (testo o vocale).
    RevealOnce(usize),
    /// Riprova l'invio di un messaggio fallito.
    Retry(usize),
    Seleziona(String),
}

pub struct Toast {
    pub text: String,
    pub at: Instant,
}

pub struct AudioState {
    pub id: String,
    pub player: AudioPlayer,
}

pub struct VideoState {
    pub id: String,
    pub player: VideoPlayer,
    pub texture: Option<TextureHandle>,
}

/// Chiamata in corso, come la vede l'interfaccia.
pub struct CallUi {
    pub peer: String,
    pub phase: crate::engine::CallPhase,
    pub video: bool,
    pub since: Instant,
    pub muted: bool,
}

pub struct App {
    pub ctx: egui::Context,
    pub paths: Paths,
    pub store: Store,
    pub engine: Option<Engine>,
    pub pair_phone: Option<String>,

    pub conn: Conn,
    pub status_text: String,
    pub qr_code: String,
    pub qr_tex: Option<TextureHandle>,
    pub logo_tex: Option<TextureHandle>,
    pub me_name: String,
    pub me_jid: String,
    pub reconnecting: bool,

    pub chats: Vec<Chat>,
    pub index: HashMap<String, usize>,
    pub selected: Option<String>,
    pub messages: Vec<Message>,
    pub search: String,

    pub composer: String,
    pub composer_focus: bool,
    pub reply: Option<ReplyTarget>,
    pub reply_label: String,
    pub typing_sent: Option<Instant>,
    pub typing_last: Instant,
    pub typing_until: HashMap<String, Instant>,

    /// Nomi noti per JID (rubrica, push name, storico).
    pub contacts: HashMap<String, Contact>,
    /// Mappatura LID → numero: le chat nate con indirizzamento LID vengono
    /// unificate sulla chat del numero, così i nomi e le foto tornano.
    pub aliases: HashMap<String, String>,
    /// Foto profilo già decodificate, per JID.
    pub avatars: HashMap<String, TextureHandle>,
    /// JID per cui abbiamo già chiesto la foto (per non ripetere la richiesta).
    pub avatar_asked: std::collections::HashSet<String>,
    pub avatar_budget: (Instant, u32),
    /// Chiamata in corso (squillo, composizione, attiva).
    pub call: Option<CallUi>,
    pub gfx: Textures,
    pub pending: HashMap<u64, Pending>,
    pub next_req: u64,
    pub actions: Vec<Action>,

    pub viewer: Option<String>,
    pub view_view_once: bool,
    /// Testo di un messaggio "una volta sola" appena svelato: (chat, id, testo).
    pub once_text: Option<(String, String, String)>,
    pub viewer_image: Option<TextureHandle>,
    pub viewer_loading: bool,
    pub video: Option<VideoState>,
    pub audio: Option<AudioState>,

    pub settings: bool,
    pub confirm_logout: bool,
    pub new_chat: bool,
    pub new_chat_phone: String,
    pub pair_modal: bool,
    pub pair_number: String,
    pub path_modal: bool,
    pub path_input: String,

    pub recorder: Option<Recorder>,
    pub rec_start: Instant,
    pub rec_path: PathBuf,

    pub attach_rx: Option<Receiver<Option<PathBuf>>>,

    pub config: Config,
    /// Diventa vera quando una seconda istanza chiede di mostrare la finestra.
    pub focus_flag: Option<Arc<AtomicBool>>,
    pub toast: Option<Toast>,
    pub last_expiry: Instant,
    pub scroll_bottom: bool,
    pub started: Instant,
}

impl App {
    /// Collega la bandierina di istanza singola: se una seconda copia parte,
    /// questa finestra torna davanti invece di restare sepolta.
    pub fn with_focus_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.focus_flag = Some(flag);
        self
    }

    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> anyhow::Result<Self> {
        let config = Config::load(&paths.data);
        crate::theme::apply(&cc.egui_ctx, config.system_fonts);
        let store = Store::open(&paths.db)?;
        // La mappatura LID ↔ numero va ripresa da disco *prima* di leggere
        // chat e contatti: è quella che tiene uniti i due indirizzi dello
        // stesso contatto tra un avvio e l'altro.
        let aliases: HashMap<String, String> = store
            .load_aliases()
            .unwrap_or_default()
            .into_iter()
            .collect();
        if let Err(error) = store.backfill_names(&aliases) {
            log::debug!("nomi dall'archivio: {error}");
        }
        let chats = store.load_chats().unwrap_or_default();
        let contacts: HashMap<String, Contact> = store
            .load_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(|contact| (contact.jid.clone(), contact))
            .collect();

        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            paths,
            store,
            engine: None,
            pair_phone: None,
            conn: Conn::Starting,
            status_text: "Connessione…".to_string(),
            qr_code: String::new(),
            qr_tex: None,
            logo_tex: None,
            me_name: String::new(),
            me_jid: String::new(),
            reconnecting: false,
            chats,
            index: HashMap::new(),
            selected: None,
            messages: Vec::new(),
            search: String::new(),
            composer: String::new(),
            composer_focus: false,
            reply: None,
            reply_label: String::new(),
            typing_sent: None,
            typing_last: Instant::now(),
            typing_until: HashMap::new(),
            contacts,
            aliases,
            avatars: HashMap::new(),
            avatar_asked: std::collections::HashSet::new(),
            avatar_budget: (Instant::now(), 0),
            call: None,
            gfx: Textures::default(),
            pending: HashMap::new(),
            next_req: 0,
            actions: Vec::new(),
            viewer: None,
            view_view_once: false,
            once_text: None,
            viewer_image: None,
            viewer_loading: false,
            video: None,
            audio: None,
            settings: false,
            confirm_logout: false,
            new_chat: false,
            new_chat_phone: String::new(),
            pair_modal: false,
            pair_number: String::new(),
            path_modal: false,
            path_input: String::new(),
            recorder: None,
            rec_start: Instant::now(),
            rec_path: PathBuf::new(),
            attach_rx: None,
            config,
            focus_flag: None,
            toast: None,
            last_expiry: Instant::now(),
            scroll_bottom: false,
            started: Instant::now(),
        };
        app.sort_chats();
        // Chat nate con indirizzamento LID: vanno unite a quelle del numero
        // anche quando la mappatura arriva da un avvio precedente.
        let pairs: Vec<(String, String)> = app
            .aliases
            .iter()
            .map(|(lid, pn)| (lid.clone(), pn.clone()))
            .collect();
        for (lid, pn) in pairs {
            app.merge_alias(&lid, &pn);
        }
        app.sort_chats();
        app.begin_engine(None);
        Ok(app)
    }

    /// Riavvia il motore chiedendo un codice di accoppiamento.
    pub fn begin_pairing(mut self, phone: String) -> Self {
        self.begin_engine(Some(phone));
        self
    }

    pub fn logged_in(&self) -> bool {
        self.conn == Conn::Connected
    }

    pub fn begin_engine(&mut self, pair: Option<String>) {
        if let Some(engine) = self.engine.take() {
            engine.stop();
        }
        self.pair_phone = pair.clone();
        self.conn = Conn::Starting;
        self.status_text = "Connessione…".to_string();
        self.qr_code.clear();
        self.qr_tex = None;
        self.reconnecting = false;

        let ctx = self.ctx.clone();
        let wake = move || ctx.request_repaint();
        self.engine = Some(Engine::start(
            self.paths.clone(),
            pair,
            Some(Box::new(wake)),
        ));
    }

    pub fn note(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            at: Instant::now(),
        });
    }

    // ── contatti, foto profilo, chiamate ─────────────────────────────────

    /// JID canonico: se conosciamo la mappatura LID → numero, usiamo il
    /// numero, così chat, nomi e foto non si sdoppiano.
    pub fn canonical(&self, jid: &str) -> String {
        let mut current = jid.to_string();
        for _ in 0..4 {
            match self.aliases.get(&current) {
                Some(next) if next != &current => current = next.clone(),
                _ => break,
            }
        }
        current
    }

    pub fn contact_for(&self, jid: &str) -> Option<&Contact> {
        let canonical = self.canonical(jid);
        if let Some(contact) = self.contacts.get(jid).or_else(|| self.contacts.get(&canonical)) {
            return Some(contact);
        }
        // Lo stesso contatto vive su due indirizzi (numero e LID): se il nome
        // è noto solo sull'altro, vale comunque.
        let twin = self.twin_of(&canonical)?;
        self.contacts.get(&twin)
    }

    /// L'altro indirizzo dello stesso contatto: `@lid` ↔ numero.
    fn twin_of(&self, jid: &str) -> Option<String> {
        if let Some(pn) = self.aliases.get(jid) {
            return Some(pn.clone());
        }
        self.aliases
            .iter()
            .find_map(|(lid, pn)| (pn == jid).then(|| lid.clone()))
    }

    /// Nome da mostrare in lista e in intestazione.
    pub fn title_for(&self, chat: &Chat) -> String {
        crate::model::contact_display(self.contact_for(&chat.id), chat)
    }

    /// Nome da mostrare per un JID che non è (ancora) una chat.
    pub fn name_of(&self, jid: &str) -> String {
        if let Some(contact) = self.contact_for(jid) {
            if !contact.name.trim().is_empty() {
                return contact.name.clone();
            }
            if !contact.phone.is_empty() {
                return crate::util::phone_display(&contact.phone);
            }
        }
        crate::util::display_name("", jid)
    }

    fn apply_contact_name(&mut self, jid: &str, name: &str, rank: u8) {
        let name = name.trim();
        if jid.is_empty() || name.is_empty() {
            return;
        }
        let jid = self.canonical(jid);
        // Alcune sorgenti (storico, notifiche) mandano il numero al posto
        // del nome: in quel caso è un recapito, non un nome da mostrare.
        if util::looks_like_phone(name) {
            let phone = name.trim_start_matches('+').to_string();
            self.store.set_contact_phone(&jid, &phone).ok();
            let entry = self
                .contacts
                .entry(jid.clone())
                .or_insert_with(|| Contact::empty(&jid));
            if entry.phone.is_empty() {
                entry.phone = phone;
            }
            return;
        }
        let name = name.to_string();
        {
            let entry = self
                .contacts
                .entry(jid.clone())
                .or_insert_with(|| Contact::empty(&jid));
            if rank >= entry.rank {
                entry.name = name.clone();
                entry.rank = rank;
            }
        }
        self.store.set_contact_name(&jid, &name, rank).ok();
        let twin = if jid.ends_with("@lid") {
            self.aliases.get(&jid).cloned()
        } else {
            self.aliases
                .iter()
                .find_map(|(lid, pn)| (pn == &jid).then(|| lid.clone()))
        };
        if let Some(twin) = twin {
            let entry = self
                .contacts
                .entry(twin.clone())
                .or_insert_with(|| Contact::empty(&twin));
            if rank >= entry.rank {
                entry.name = name.clone();
                entry.rank = rank;
            }
            self.store.set_contact_name(&twin, &name, rank).ok();
        }
    }

    /// Unisce una chat nata con indirizzamento LID alla chat del numero.
    fn adopt_alias(&mut self, lid: &str, pn: &str) {
        if lid.is_empty() || pn.is_empty() || lid == pn {
            return;
        }
        if self.aliases.get(lid).map(String::as_str) == Some(pn) {
            return;
        }
        self.aliases.insert(lid.to_string(), pn.to_string());
        self.store.save_alias(lid, pn).ok();
        self.merge_alias(lid, pn);
    }

    /// Fonde i dati dei due indirizzi: contatti, foto e chat.
    fn merge_alias(&mut self, lid: &str, pn: &str) {
        if lid.is_empty() || pn.is_empty() || lid == pn {
            return;
        }
        let phone = crate::util::jid_user(pn);
        let lid_contact = self.contacts.get(lid).cloned();
        let existing = self.contacts.get(pn).cloned();
        let mut merged = existing.unwrap_or_else(|| Contact::empty(pn));
        if let Some(other) = lid_contact.as_ref() {
            if merged.name.trim().is_empty() && !other.name.trim().is_empty() {
                merged.name = other.name.clone();
                merged.rank = other.rank;
            }
            if merged.avatar.is_none() {
                merged.avatar = other.avatar.clone();
            }
        }
        merged.phone = phone;
        self.store.save_contact(&merged, 0).ok();
        self.contacts.insert(pn.to_string(), merged.clone());
        self.contacts.insert(lid.to_string(), merged.clone());
        if let Some(bytes) = merged.avatar.as_ref() {
            self.ensure_avatar_texture(pn, bytes);
            self.ensure_avatar_texture(lid, bytes);
        }

        self.store.rename_jid(lid, pn).ok();
        if let Some(index) = self.index.get(lid).copied() {
            let old = self.chats.remove(index);
            self.sort_chats();
            if self.index.contains_key(pn) {
                self.store.delete_chat(lid).ok();
                if let Some(index) = self.index.get(pn).copied() {
                    self.chats[index].unread += old.unread;
                }
            } else {
                let mut chat = old;
                chat.id = pn.to_string();
                self.store.upsert_chat(&chat).ok();
                self.chats.push(chat);
                self.sort_chats();
            }
            if self.selected.as_deref() == Some(lid) {
                self.selected = Some(pn.to_string());
                self.messages = self.store.load_messages(pn, 300).unwrap_or_default();
            }
        }
    }

    fn ensure_avatar_texture(&mut self, jid: &str, bytes: &[u8]) {
        if self.avatars.contains_key(jid) {
            return;
        }
        let Some((image, _, _)) = gfx::image_from_bytes(bytes) else {
            return;
        };
        let texture = self.ctx.load_texture(
            format!("avatar:{jid}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.avatars.insert(jid.to_string(), texture);
    }

    fn apply_avatar(&mut self, jid: &str, bytes: Option<Vec<u8>>) {
        let jid = self.canonical(jid);
        if jid.is_empty() {
            return;
        }
        self.store
            .set_contact_avatar(&jid, bytes.as_deref(), crate::util::now_ts())
            .ok();
        {
            let entry = self
                .contacts
                .entry(jid.clone())
                .or_insert_with(|| Contact::empty(&jid));
            entry.avatar = bytes.clone();
        }
        match bytes.as_ref() {
            Some(bytes) => {
                // Una foto nuova sostituisce quella vecchia.
                self.avatars.remove(&jid);
                self.ensure_avatar_texture(&jid, bytes);
            }
            None => {
                // Nessuna foto: non richiederla più per questa sessione.
                self.avatar_asked.insert(jid.clone());
                self.avatars.remove(&jid);
            }
        }
        let twin = if jid.ends_with("@lid") {
            self.aliases.get(&jid).cloned()
        } else {
            self.aliases
                .iter()
                .find_map(|(lid, pn)| (pn == &jid).then(|| lid.clone()))
        };
        if let Some(twin) = twin {
            if let Some(entry) = self.contacts.get_mut(&twin) {
                entry.avatar = bytes.clone();
            }
            match bytes.as_ref() {
                Some(bytes) => self.ensure_avatar_texture(&twin, bytes),
                None => {
                    self.avatars.remove(&twin);
                }
            }
        }
    }

    /// Chiede la foto profilo, con un piccolo limite di richieste al secondo.
    pub fn request_avatar(&mut self, jid: &str) {
        let jid = self.canonical(jid);
        if jid.is_empty() || self.avatars.contains_key(&jid) {
            return;
        }
        // Foto già scaricata in una sessione precedente: la texture va
        // ricreata al primo disegno, altrimenti dopo ogni avvio sparisce.
        if let Some(bytes) = self
            .contacts
            .get(&jid)
            .and_then(|contact| contact.avatar.clone())
        {
            self.ensure_avatar_texture(&jid, &bytes);
            if !self.avatars.contains_key(&jid) {
                // Bytes illeggibili: non riprovare a ogni fotogramma.
                self.avatar_asked.insert(jid);
            }
            return;
        }
        if self.avatar_asked.contains(&jid) {
            return;
        }
        let (stamp, count) = self.avatar_budget;
        let count = if stamp.elapsed() > Duration::from_secs(1) {
            0
        } else {
            count
        };
        if count >= 3 {
            return;
        }
        self.avatar_budget = (Instant::now(), count + 1);
        self.avatar_asked.insert(jid.clone());
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        engine.send(Cmd::Avatar { jid });
    }

    fn apply_call(&mut self, phase: crate::engine::CallPhase, peer: String, video: bool) {
        use crate::engine::CallPhase;
        if phase == CallPhase::Ended {
            if self.call.is_some() {
                self.note("Chiamata terminata");
            }
            self.call = None;
            return;
        }
        let previous = self.call.as_ref();
        let peer = if peer.is_empty() {
            previous.map(|call| call.peer.clone()).unwrap_or_default()
        } else {
            self.canonical(&peer)
        };
        let muted = previous.map(|call| call.muted).unwrap_or(false);
        let since = match (previous, phase) {
            (Some(call), phase) if phase != CallPhase::Active => call.since,
            _ => Instant::now(),
        };
        self.call = Some(CallUi {
            peer,
            phase,
            video,
            since,
            muted,
        });
    }

    fn apply_missed_call(&mut self, peer: String, video: bool) {
        let label = if video {
            "Videochiamata persa"
        } else {
            "Chiamata persa"
        };
        let chat_id = self.canonical(&peer);
        if chat_id.is_empty() {
            return;
        }
        let message = Message {
            id: format!("call-{}", crate::util::timestamp_ms()),
            chat_id,
            ts: crate::util::now_ts(),
            kind: Kind::System,
            text: label.to_string(),
            ..Default::default()
        };
        self.ingest(message);
        let who = self.name_of(&peer);
        self.note(format!("{label} · {who}"));
    }

    pub fn start_call(&mut self) {
        let Some(chat) = self.selected.clone() else {
            return;
        };
        if self.call.is_some() {
            self.note("Sei già in una chiamata");
            return;
        }
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        // Nessuno stato inventato: il pannello compare quando il motore ha
        // davvero aperto microfono e casse e sta mandando l'offerta.
        engine.send(Cmd::CallStart { chat: chat.clone() });
        let name = self.name_of(&chat);
        self.note(format!("Chiamata a {name}…"));
    }

    pub fn sort_chats(&mut self) {
        self.chats
            .sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last_ts.cmp(&a.last_ts)));
        self.index.clear();
        for (i, chat) in self.chats.iter().enumerate() {
            self.index.insert(chat.id.clone(), i);
        }
    }

    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.index.get(id).and_then(|i| self.chats.get(*i))
    }

    pub fn selected_chat(&self) -> Option<&Chat> {
        self.selected.as_deref().and_then(|id| self.chat(id))
    }

    pub fn drain_events(&mut self) {
        let mut events = Vec::new();
        if let Some(engine) = self.engine.as_ref() {
            while let Ok(event) = engine.events.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Qr { code } => {
                self.conn = Conn::Qr;
                self.status_text = "In attesa del telefono".to_string();
                if self.qr_code != code {
                    self.qr_code = code;
                    self.qr_tex = None;
                }
            }
            Event::PairCode { code } => {
                self.conn = Conn::PairCode;
                self.qr_tex = None;
                self.qr_code = code;
            }
            Event::PairError(error) => {
                self.status_text = format!("Accoppiamento non riuscito: {error}");
            }
            Event::Me { name, jid } => {
                if !name.is_empty() {
                    self.me_name = name;
                }
                if !jid.is_empty() {
                    self.me_jid = jid;
                }
            }
            Event::Connected => {
                let first = self.conn != Conn::Connected;
                self.conn = Conn::Connected;
                self.reconnecting = false;
                self.status_text.clear();
                if first {
                    self.chats = self.store.load_chats().unwrap_or_default();
                    self.sort_chats();
                    self.note("Connesso");
                }
            }
            Event::Disconnected(reason) => {
                self.reconnecting = true;
                self.status_text = reason;
            }
            Event::LoggedOut => {
                self.reset_session();
            }
            Event::Fatal(error) => {
                self.conn = Conn::Error;
                self.status_text = error;
            }
            Event::Messages(messages) => {
                for message in messages {
                    self.ingest(message);
                }
            }
            Event::History { chats, messages, done } => {
                for mut chat in chats {
                    chat.id = self.canonical(&chat.id);
                    if !self.index.contains_key(&chat.id) {
                        let _ = self.store.upsert_chat(&chat);
                        self.chats.push(chat);
                    }
                }
                self.sort_chats();
                let messages: Vec<Message> = messages
                    .into_iter()
                    .map(|mut message| {
                        message.chat_id = self.canonical(&message.chat_id);
                        message
                    })
                    .collect();
                self.store.insert_messages(&messages).ok();
                if let Some(selected) = self.selected.clone() {
                    self.messages = self.store.load_messages(&selected, 300).unwrap_or_default();
                }
                if done {
                    self.note("Cronologia importata");
                }
            }
            Event::ChatName { chat, name } => {
                let chat = self.canonical(&chat);
                if let Some(index) = self.index.get(&chat).copied() {
                    self.chats[index].name = name.clone();
                    self.store.set_chat_name(&chat, &name).ok();
                }
                if !crate::util::jid_is_group(&chat) {
                    self.apply_contact_name(&chat, &name, 3);
                }
            }
            Event::Status { chat, ids, status } => {
                let chat = self.canonical(&chat);
                for message in self.messages.iter_mut() {
                    if message.chat_id == chat && ids.iter().any(|id| id == &message.id) {
                        if status > message.status {
                            message.status = status;
                        }
                    }
                }
                for id in &ids {
                    self.store.update_status(&chat, id, status).ok();
                }
            }
            Event::Typing { chat, typing } => {
                let chat = self.canonical(&chat);
                if let Some(index) = self.index.get(&chat).copied() {
                    self.chats[index].typing = typing;
                }
                if typing {
                    self.typing_until
                        .insert(chat, Instant::now() + Duration::from_secs(7));
                } else {
                    self.typing_until.remove(&chat);
                }
            }
            Event::Presence {
                chat,
                online,
                last_seen,
            } => {
                let chat = self.canonical(&chat);
                if let Some(index) = self.index.get(&chat).copied() {
                    self.chats[index].online = online;
                    if last_seen.is_some() {
                        self.chats[index].last_seen = last_seen;
                    }
                }
            }
            Event::Contact { jid, name, rank } => {
                self.apply_contact_name(&jid, &name, rank);
            }
            Event::Alias { lid, pn } => {
                self.adopt_alias(&lid, &pn);
            }
            Event::Avatar { jid, bytes } => {
                self.apply_avatar(&jid, bytes);
            }
            Event::Call { phase, peer, video } => {
                self.apply_call(phase, peer, video);
            }
            Event::CallMissed { peer, video } => {
                self.apply_missed_call(peer, video);
            }
            Event::Notice(text) => {
                self.note(text);
            }
            Event::Sent {
                chat,
                local_id,
                server_id,
                ts,
                error,
            } => {
                if let Some(server_id) = server_id {
                    if let Some(index) = self
                        .messages
                        .iter()
                        .position(|message| message.id == local_id)
                    {
                        self.messages[index].id = server_id.clone();
                        self.messages[index].ts = ts;
                        self.messages[index].status = Status::Sent;
                        self.scroll_bottom = true;
                    }
                    self.gfx
                        .rename(&format!("local:{local_id}"), &format!("local:{server_id}"));
                    self.store
                        .rename_message_id(&chat, &local_id, &server_id, ts)
                        .ok();
                } else if let Some(index) = self
                    .messages
                    .iter()
                    .position(|message| message.id == local_id)
                {
                    self.messages[index].status = Status::Failed;
                    self.store
                        .update_status(&chat, &local_id, Status::Failed)
                        .ok();
                    let reason = error.unwrap_or_else(|| "invio non riuscito".to_string());
                    self.note(format!("Messaggio non inviato: {reason}"));
                }
            }
            Event::Media {
                req,
                chat,
                id,
                result,
            } => {
                let Some(pending) = self.pending.remove(&req) else {
                    return;
                };
                match result {
                    Ok(bytes) => self.media_ready(pending, bytes),
                    Err(error) => {
                        self.note(format!("Download non riuscito: {error}"));
                        let _ = (chat, id);
                    }
                }
            }
            Event::Edited { chat, id, text } => {
                if let Some(message) = self
                    .messages
                    .iter_mut()
                    .find(|message| message.id == id && message.chat_id == chat)
                {
                    message.text = text.clone();
                    message.edited = true;
                }
                self.store
                    .apply_edit(&chat, &id, &text, crate::util::now_ts())
                    .ok();
            }
            Event::Revoked { chat, id } => {
                if let Some(message) = self
                    .messages
                    .iter_mut()
                    .find(|message| message.id == id && message.chat_id == chat)
                {
                    message.deleted = true;
                    message.text.clear();
                    message.payload.clear();
                    message.thumb = None;
                }
                self.store.apply_revoke(&chat, &id).ok();
            }
            Event::Reaction { chat, id, emoji } => {
                if let Some(message) = self
                    .messages
                    .iter_mut()
                    .find(|message| message.id == id && message.chat_id == chat)
                {
                    message.reaction = emoji.clone();
                }
                self.store.set_reaction(&chat, &id, emoji.as_deref()).ok();
            }
            Event::Undecryptable => {
                self.note("Un messaggio non è decifrabile su questo dispositivo");
            }
        }
    }

    pub fn reset_session(&mut self) {
        if let Some(engine) = self.engine.take() {
            engine.stop();
        }
        self.chats.clear();
        self.index.clear();
        self.messages.clear();
        self.contacts.clear();
        self.aliases.clear();
        self.avatars.clear();
        self.avatar_asked.clear();
        self.call = None;
        self.selected = None;
        self.gfx.clear();
        self.pending.clear();
        self.store.wipe().ok();
        let _ = std::fs::remove_file(&self.paths.session);
        let _ = std::fs::remove_file(self.paths.session.with_extension("db-wal"));
        let _ = std::fs::remove_file(self.paths.session.with_extension("db-shm"));
        let _ = std::fs::remove_dir_all(&self.paths.media);
        let _ = std::fs::create_dir_all(&self.paths.media);
        self.begin_engine(None);
    }

    fn ingest(&mut self, mut message: Message) {
        message.chat_id = self.canonical(&message.chat_id);
        let chat_id = message.chat_id.clone();
        // Eco di un messaggio inviato da questa app: invece di aggiungere una
        // seconda bolla, adotta l'id del server su quella locale (che è già
        // a schermo). Vale anche se il server risponde prima di `Event::Sent`.
        if message.from_me && !message.text.is_empty() && message.kind == Kind::Text {
            if let Some(local) = self.store.find_pending(&chat_id, &message.text, message.ts) {
                self.store
                    .rename_message_id(&chat_id, &local, &message.id, message.ts)
                    .ok();
                if let Some(index) = self.messages.iter().position(|item| item.id == local) {
                    self.messages[index].id = message.id.clone();
                    self.messages[index].ts = message.ts;
                    self.messages[index].status = Status::Sent;
                }
                self.gfx.rename(
                    &format!("local:{local}"),
                    &format!("local:{}", message.id),
                );
            }
        }
        if util::jid_is_status(&chat_id) || util::jid_is_newsletter(&chat_id) || chat_id.is_empty() {
            return;
        }

        if !self.index.contains_key(&chat_id) {
            let is_group = util::jid_is_group(&chat_id);
            let name = if is_group {
                String::new()
            } else {
                message.sender_name.clone()
            };
            let chat = Chat {
                id: chat_id.clone(),
                name,
                is_group,
                last_ts: message.ts,
                preview: message.preview(),
                ..Default::default()
            };
            let _ = self.store.upsert_chat(&chat);
            self.chats.push(chat);
            if is_group {
                if let Some(engine) = self.engine.as_ref() {
                    engine.send(Cmd::GroupInfo {
                        chat: chat_id.clone(),
                    });
                }
            }
        }

        let selected = self.selected.as_deref() == Some(chat_id.as_str());
        let count = if !selected && !message.from_me { 1 } else { 0 };

        let _ = self.store.insert_message(&message);

        if selected {
            if !self.messages.iter().any(|existing| existing.id == message.id) {
                self.messages.push(message.clone());
                self.scroll_bottom = true;
            }
            if !message.from_me && !message.id.is_empty() {
                let sender = if util::jid_is_group(&chat_id) {
                    Some(message.sender.clone())
                } else {
                    None
                };
                if let Some(engine) = self.engine.as_ref() {
                    engine.send(Cmd::MarkRead {
                        chat: chat_id.clone(),
                        sender,
                        ids: vec![message.id.clone()],
                    });
                }
            }
        }

        if let Some(index) = self.index.get(&chat_id).copied() {
            let chat = &mut self.chats[index];
            if message.ts >= chat.last_ts {
                chat.last_ts = message.ts;
                chat.preview = message.preview();
            }
            if count > 0 {
                chat.unread = chat.unread.saturating_add(1);
                self.store.bump_unread(&chat_id).ok();
            }
            let preview = chat.preview.clone();
            self.store.touch_chat(&chat_id, message.ts, &preview).ok();
        }
        self.sort_chats();

        if count > 0 && self.selected.as_deref() != Some(chat_id.as_str()) {
            let title = self
                .chat(&chat_id)
                .map(|chat| self.title_for(chat))
                .unwrap_or_default();
            self.note(format!("Nuovo messaggio da {title}"));
        }
    }

    pub fn select_chat(&mut self, id: String) {
        if self.selected.as_deref() == Some(id.as_str()) {
            return;
        }
        self.selected = Some(id.clone());
        self.messages = self
            .store
            .load_messages(&id, 300)
            .unwrap_or_default();
        self.scroll_bottom = true;
        self.gfx.clear();
        self.reply = None;
        self.reply_label.clear();
        self.store.mark_read(&id).ok();
        if let Some(index) = self.index.get(&id).copied() {
            self.chats[index].unread = 0;
        }
        if let Some(engine) = self.engine.as_ref() {
            if let Ok(ids) = self.store.unread_ids(&id) {
                if !ids.is_empty() {
                    let sender = ids.iter().find_map(|(_, sender)| {
                        if sender.is_empty() {
                            None
                        } else {
                            Some(sender.clone())
                        }
                    });
                    engine.send(Cmd::MarkRead {
                        chat: id,
                        sender,
                        ids: ids.into_iter().map(|(id, _)| id).collect(),
                    });
                }
            }
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        // Presenza "sta scrivendo" scaduta.
        let now = Instant::now();
        let mut expired = Vec::new();
        for (chat, until) in self.typing_until.iter() {
            if *until < now {
                expired.push(chat.clone());
            }
        }
        for chat in expired {
            self.typing_until.remove(&chat);
            if let Some(index) = self.index.get(&chat).copied() {
                self.chats[index].typing = false;
            }
        }

        // Messaggi effimeri scaduti.
        if now.duration_since(self.last_expiry) > Duration::from_secs(20) {
            self.last_expiry = now;
            let ts = crate::util::now_ts();
            self.store.delete_expired(ts).ok();
            self.messages
                .retain(|message| message.expire_ts == 0 || message.expire_ts > ts);
        }

        // Risveglio della forma d'onda / indicatore di scrittura.
        if self.recorder.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        } else if self
            .typing_until
            .values()
            .any(|until| *until > Instant::now())
        {
            ctx.request_repaint_after(Duration::from_millis(700));
        }
        if self.conn == Conn::Starting {
            ctx.request_repaint_after(Duration::from_millis(160));
        }
        if self.conn == Conn::Qr || self.conn == Conn::PairCode {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        // Registrazione: stop automatico dopo due minuti.
        if let Some(seconds) = self.recorder.as_ref().map(|recorder| recorder.seconds()) {
            if seconds >= 120.0 {
                self.stop_recording(true);
            }
        }

        // Audio in riproduzione.
        let mut audio_done = false;
        if let Some(audio) = self.audio.as_mut() {
            if audio.player.finished() {
                audio_done = true;
            } else {
                ctx.request_repaint_after(Duration::from_millis(90));
            }
        }
        if audio_done {
            if let Some(audio) = self.audio.take() {
                self.mark_played(&audio.id, false);
            }
        }

        // Video in riproduzione.
        let mut video_frame: Option<(String, usize, usize)> = None;
        if let Some(video) = self.video.as_mut() {
            // Le misure vanno lette prima: il fotogramma resta in prestito.
            let (width, height) = (video.player.width as usize, video.player.height as usize);
            if let Some(frame) = video.player.poll_frame() {
                if width > 0 && height > 0 && frame.len() == width * height * 3 {
                    let mut rgba = Vec::with_capacity(width * height * 4);
                    for chunk in frame.chunks_exact(3) {
                        rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
                    }
                    let image =
                        egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    match video.texture.as_mut() {
                        Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                        None => {
                            video.texture = Some(ctx.load_texture(
                                format!("video:{}", video.id),
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                    }
                    video_frame = Some((video.id.clone(), width, height));
                }
            }
            if video.player.looping && video.player.at_end() {
                video.player.seek(0.0);
                video.player.play();
            }
            if video.player.playing || video.player.position() + 0.5 < video.player.duration {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        let _ = video_frame;

        // File scelti dal selettore.
        if let Some(rx) = self.attach_rx.take() {
            match rx.try_recv() {
                Ok(Some(path)) => self.send_file(path, None),
                Ok(None) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.attach_rx = Some(rx);
                    ctx.request_repaint_after(Duration::from_millis(120));
                }
            }
        }
    }

    /// File trascinati nella finestra: si inviano alla chat aperta.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if !self.logged_in() || self.selected.is_none() {
            return;
        }
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| Some(file.path().to_path_buf()))
                .collect()
        });
        for path in dropped.into_iter().take(10) {
            self.send_file(path, None);
        }
    }

    pub fn apply_actions(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            match action {
                Action::OpenImage(index) => self.open_image(index),
                Action::OpenVideo(index) => self.open_video(index),
                Action::PlayAudio(index) => self.toggle_audio(index),
                Action::OpenDoc(index) => self.open_document(index),
                Action::Download(index) => self.download_message(index),
                Action::RevealOnce(index) => {
                    let Some(message) = self.messages.get(index).cloned() else {
                        continue;
                    };
                    if message.played {
                        continue;
                    }
                    // Il contenuto si apre con il suo lettore: il consumo
                    // avviene quando è davvero a schermo (mark_played).
                    match message.kind {
                        Kind::Text => {
                            self.once_text = Some((
                                message.chat_id.clone(),
                                message.id.clone(),
                                message.text.clone(),
                            ));
                            self.mark_played(&message.id, true);
                        }
                        Kind::Image | Kind::Sticker => self.open_image(index),
                        Kind::Video | Kind::Gif => self.open_video(index),
                        Kind::Voice | Kind::Audio => self.toggle_audio(index),
                        Kind::Document => self.open_document(index),
                        _ => self.download_message(index),
                    }
                }
                Action::Reply(index) => {
                    if let Some(message) = self.messages.get(index).cloned() {
                        self.reply = Some(ReplyTarget {
                            id: message.id.clone(),
                            sender: if message.from_me {
                                String::new()
                            } else {
                                message.sender.clone()
                            },
                            payload: message.payload.clone(),
                        });
                        let who = if message.from_me {
                            "te stesso".to_string()
                        } else {
                            self.name_of(&message.sender)
                        };
                        self.reply_label = format!("{who}: {}", message.preview());
                        self.composer_focus = true;
                    }
                }
                Action::React(index, emoji) => {
                    if let Some(message) = self.messages.get(index).cloned() {
                        let emoji = if message.reaction.as_deref() == Some(emoji.as_str()) {
                            String::new()
                        } else {
                            emoji
                        };
                        if let Some(engine) = self.engine.as_ref() {
                            engine.send(Cmd::React {
                                chat: message.chat_id.clone(),
                                id: message.id.clone(),
                                from_me: message.from_me,
                                participant: if message.from_me {
                                    None
                                } else {
                                    Some(message.sender.clone())
                                },
                                emoji,
                            });
                        }
                    }
                }
                Action::ClearReaction(index) => {
                    if let Some(message) = self.messages.get(index).cloned() {
                        if let Some(engine) = self.engine.as_ref() {
                            engine.send(Cmd::React {
                                chat: message.chat_id.clone(),
                                id: message.id.clone(),
                                from_me: message.from_me,
                                participant: if message.from_me {
                                    None
                                } else {
                                    Some(message.sender.clone())
                                },
                                emoji: String::new(),
                            });
                        }
                    }
                }
                Action::Retry(index) => {
                    let Some(message) = self.messages.get(index).cloned() else {
                        continue;
                    };
                    if message.kind != Kind::Text || message.text.is_empty() {
                        self.note("Questo tipo di messaggio va re-inviato a mano");
                        continue;
                    }
                    self.store.retry_message(&message.chat_id, &message.id).ok();
                    if let Some(entry) = self.messages.get_mut(index) {
                        entry.status = Status::Pending;
                    }
                    if let Some(engine) = self.engine.as_ref() {
                        engine.send(Cmd::SendText {
                            chat: message.chat_id.clone(),
                            text: message.text.clone(),
                            local_id: message.id.clone(),
                            reply: None,
                        });
                    }
                }
                Action::CopyText(index) => {
                    if let Some(message) = self.messages.get(index) {
                        self.ctx.copy_text(message.text.clone());
                        self.note("Testo copiato");
                    }
                }
                Action::Revoke(index) => {
                    if let Some(message) = self.messages.get(index).cloned() {
                        if let Some(engine) = self.engine.as_ref() {
                            engine.send(Cmd::Revoke {
                                chat: message.chat_id.clone(),
                                id: message.id.clone(),
                                from_me: message.from_me,
                                sender: Some(message.sender.clone()),
                            });
                        }
                    }
                }
                Action::Seleziona(chat) => self.select_chat(chat),
            }
        }
    }

    pub fn mark_played(&mut self, id: &str, send_receipt: bool) {
        if let Some(message) = self.messages.iter_mut().find(|message| message.id == id) {
            message.played = true;
            let chat = message.chat_id.clone();
            let sender = if util::jid_is_group(&chat) {
                Some(message.sender.clone())
            } else {
                None
            };
            self.store.set_played(&chat, id).ok();
            // Un contenuto "una volta sola" viene consumato appena aperto:
            // dopo non è più leggibile né dall'archivio né in memoria.
            if message.view_once {
                self.store.consume_once(&chat, id).ok();
                message.text.clear();
                message.payload.clear();
                message.thumb = None;
                message.local_preview = None;
            }
            if send_receipt {
                if let Some(engine) = self.engine.as_ref() {
                    engine.send(Cmd::Played {
                        chat,
                        sender,
                        ids: vec![id.to_string()],
                    });
                }
            }
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some(toast) = self.toast.as_ref() else {
            return;
        };
        if toast.at.elapsed() > Duration::from_secs(4) {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Palette::SURFACE)
                    .stroke(egui::Stroke::new(1.0, Palette::BORDER))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&toast.text).color(Palette::TEXT).size(12.5));
                    });
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ctx = ctx.clone();
        if let Some(flag) = self.focus_flag.as_ref() {
            if flag.swap(false, Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
        }
        self.drain_events();
        self.tick(&ctx);
        self.handle_dropped_files(&ctx);

        if self.logged_in() {
            egui::Panel::left("chats")
                .default_size(322.0)
                .min_size(248.0)
                .max_size(430.0)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .fill(Palette::PANEL)
                        .inner_margin(egui::Margin::ZERO),
                )
                .show(ui, |ui| self.sidebar_ui(ui));

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(Palette::BG)
                        .inner_margin(egui::Margin::ZERO),
                )
                .show(ui, |ui| self.chat_ui(ui));
        } else {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(Palette::BG)
                        .inner_margin(egui::Margin::ZERO),
                )
                .show(ui, |ui| self.login_ui(ui));
        }

        self.apply_actions();
        self.overlays(&ctx);
        self.toast_ui(&ctx);
    }

    /// Chiusura pulita: la sessione salva chiavi e stato prima di sparire.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(recorder) = self.recorder.take() {
            recorder.cancel();
        }
        if let Some(audio) = self.audio.take() {
            drop(audio);
        }
        if let Some(video) = self.video.take() {
            drop(video);
        }
        if self.config.graceful_exit {
            if let Some(engine) = self.engine.take() {
                engine.stop();
            }
        }
          log::info!("chiusura completata");
    }
}

pub fn color_for_status(status: Status) -> Color32 {
    match status {
        Status::Read | Status::Played => Palette::WHITE,
        Status::Failed => Palette::DANGER,
        _ => Palette::MUTED,
    }
}

pub fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => "Foto",
        Kind::Gif => "GIF",
        Kind::Video => "Video",
        Kind::Voice => "Vocale",
        Kind::Audio => "Audio",
        Kind::Document => "Documento",
        Kind::Sticker => "Sticker",
        Kind::Location => "Posizione",
        Kind::Contact => "Contatto",
        Kind::System => "Sistema",
        Kind::Other => "Messaggio",
        Kind::Text => "",
    }
}
