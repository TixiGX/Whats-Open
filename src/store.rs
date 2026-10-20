//! Archivio locale (SQLite) di chat e messaggi.
//!
//! Il database della sessione (credenziali, chiavi) è separato: lo gestisce la
//! libreria del protocollo in `session.db`. Qui vivono solo i dati visibili.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{Chat, Contact, Kind, Message, Status};

/// `true` se la tabella ha già la colonna: serve alle migrazioni leggere
/// (`ALTER TABLE ... ADD COLUMN` non è idempotente).
fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    let found = rows.filter_map(|row| row.ok()).any(|name| name == column);
    found
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS chats (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL DEFAULT '',
    is_group  INTEGER NOT NULL DEFAULT 0,
    last_ts   INTEGER NOT NULL DEFAULT 0,
    preview   TEXT NOT NULL DEFAULT '',
    unread    INTEGER NOT NULL DEFAULT 0,
    muted     INTEGER NOT NULL DEFAULT 0,
    pinned    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    id          TEXT NOT NULL,
    chat_id     TEXT NOT NULL,
    sender      TEXT NOT NULL DEFAULT '',
    sender_name TEXT NOT NULL DEFAULT '',
    from_me     INTEGER NOT NULL DEFAULT 0,
    ts          INTEGER NOT NULL DEFAULT 0,
    kind        TEXT NOT NULL DEFAULT 'text',
    body        TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'sent',
    view_once   INTEGER NOT NULL DEFAULT 0,
    played      INTEGER NOT NULL DEFAULT 0,
    payload     BLOB,
    thumb       BLOB,
    mime        TEXT NOT NULL DEFAULT '',
    file_name   TEXT NOT NULL DEFAULT '',
    file_size   INTEGER NOT NULL DEFAULT 0,
    duration    INTEGER NOT NULL DEFAULT 0,
    quote_id    TEXT NOT NULL DEFAULT '',
    quote_name  TEXT NOT NULL DEFAULT '',
    quote_text  TEXT NOT NULL DEFAULT '',
    edited      INTEGER NOT NULL DEFAULT 0,
    deleted     INTEGER NOT NULL DEFAULT 0,
    expire_ts   INTEGER NOT NULL DEFAULT 0,
    reaction    TEXT,
    PRIMARY KEY (chat_id, id)
);

CREATE INDEX IF NOT EXISTS idx_messages_chat_ts ON messages (chat_id, ts);

CREATE TABLE IF NOT EXISTS contacts (
    jid       TEXT PRIMARY KEY,
    name      TEXT NOT NULL DEFAULT '',
    name_rank INTEGER NOT NULL DEFAULT 0,
    phone     TEXT NOT NULL DEFAULT '',
    avatar    BLOB,
    avatar_ts INTEGER NOT NULL DEFAULT 0,
    checked   INTEGER NOT NULL DEFAULT 0
);

-- Lo stesso contatto ha due indirizzi (numero e LID). La mappatura va
-- ricordata: senza, a ogni avvio chat, nomi e foto si sdoppiano.
CREATE TABLE IF NOT EXISTS aliases (
    lid TEXT PRIMARY KEY,
    pn  TEXT NOT NULL
);
"#;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("apertura database {}", path.display()))?;
        conn.execute_batch(SCHEMA).context("creazione schema")?;
        // Colonne aggiunte dopo la prima release: `ALTER TABLE` solo se
        // mancano davvero, così i database già esistenti si aggiornano da soli.
        if !has_column(&conn, "messages", "animated") {
            conn.execute(
                "ALTER TABLE messages ADD COLUMN animated INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .ok();
        }
        Ok(Self { conn })
    }

    pub fn load_chats(&self) -> Result<Vec<Chat>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_group, last_ts, preview, unread, muted, pinned
             FROM chats ORDER BY pinned DESC, last_ts DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Chat {
                id: row.get(0)?,
                name: row.get(1)?,
                is_group: row.get::<_, i64>(2)? != 0,
                last_ts: row.get(3)?,
                preview: row.get(4)?,
                unread: row.get::<_, i64>(5)?.max(0) as u32,
                muted: row.get::<_, i64>(6)? != 0,
                pinned: row.get::<_, i64>(7)? != 0,
                typing: false,
                online: false,
                last_seen: None,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn load_messages(&self, chat_id: &str, limit: usize) -> Result<Vec<Message>> {
        let limit = limit.max(1) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, chat_id, sender, sender_name, from_me, ts, kind, body, status,
                    view_once, played, payload, thumb, mime, file_name, file_size,
                    duration, quote_id, quote_name, quote_text, edited, deleted, expire_ts,
                    reaction, animated
             FROM messages WHERE chat_id = ?1 ORDER BY ts DESC, rowid DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![chat_id, limit], |row| {
            Ok(Message {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                sender: row.get(2)?,
                sender_name: row.get(3)?,
                from_me: row.get::<_, i64>(4)? != 0,
                ts: row.get(5)?,
                kind: Kind::from_str(&row.get::<_, String>(6)?),
                text: row.get(7)?,
                status: Status::from_str(&row.get::<_, String>(8)?),
                view_once: row.get::<_, i64>(9)? != 0,
                played: row.get::<_, i64>(10)? != 0,
                payload: row.get::<_, Option<Vec<u8>>>(11)?.unwrap_or_default(),
                thumb: row.get::<_, Option<Vec<u8>>>(12)?,
                mime: row.get(13)?,
                file_name: row.get(14)?,
                file_size: row.get::<_, i64>(15)?.max(0) as u64,
                duration: row.get::<_, i64>(16)?.max(0) as u32,
                quote: {
                    let id: String = row.get(17)?;
                    let name: String = row.get(18)?;
                    let text: String = row.get(19)?;
                    if id.is_empty() {
                        None
                    } else {
                        Some(crate::model::Quote { id, sender: name, text })
                    }
                },
                edited: row.get::<_, i64>(20)? != 0,
                deleted: row.get::<_, i64>(21)? != 0,
                expire_ts: row.get(22)?,
                reaction: row.get::<_, Option<String>>(23)?,
                animated: row.get::<_, i64>(24)? != 0,
                local_preview: None,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn upsert_chat(&self, chat: &Chat) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chats (id, name, is_group, last_ts, preview, unread, muted, pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE chats.name END,
                is_group = excluded.is_group,
                last_ts = MAX(chats.last_ts, excluded.last_ts),
                preview = CASE WHEN excluded.last_ts >= chats.last_ts
                               THEN excluded.preview ELSE chats.preview END,
                unread = excluded.unread,
                muted = excluded.muted,
                pinned = excluded.pinned",
            params![
                chat.id,
                chat.name,
                chat.is_group as i64,
                chat.last_ts,
                chat.preview,
                chat.unread as i64,
                chat.muted as i64,
                chat.pinned as i64
            ],
        )?;
        Ok(())
    }

    /// Sposta una chat (e i suoi messaggi) da un JID a un altro: serve
    /// quando scopriamo che una conversazione aperta con un LID è in realtà
    /// lo stesso contatto del numero di telefono.
    pub fn rename_jid(&self, old_id: &str, new_id: &str) -> Result<()> {
        if old_id.is_empty() || new_id.is_empty() || old_id == new_id {
            return Ok(());
        }
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chats WHERE id = ?1",
                params![old_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let target: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chats WHERE id = ?1",
                params![new_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if exists > 0 {
            if target > 0 {
                // Destinazione già presente: la vecchia riga è un doppione.
                self.conn
                    .execute("DELETE FROM chats WHERE id = ?1", params![old_id])?;
            } else {
                self.conn
                    .execute("UPDATE chats SET id = ?2 WHERE id = ?1", params![old_id, new_id])?;
            }
        }
        // I messaggi si spostano anche quando la chat non c'è (storico
        // importato prima che la mappatura fosse nota).
        self.conn.execute(
            "UPDATE OR IGNORE messages SET chat_id = ?2 WHERE chat_id = ?1",
            params![old_id, new_id],
        )?;
        self.conn.execute(
            "DELETE FROM messages WHERE chat_id = ?1",
            params![old_id],
        )?;
        Ok(())
    }

    pub fn delete_chat(&self, chat_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM chats WHERE id = ?1", params![chat_id])?;
        Ok(())
    }

    pub fn set_chat_name(&self, chat_id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chats SET name = ?2 WHERE id = ?1",
            params![chat_id, name],
        )?;
        Ok(())
    }

    pub fn insert_message(&self, message: &Message) -> Result<bool> {
        let payload: Option<&[u8]> = if message.payload.is_empty() {
            None
        } else {
            Some(message.payload.as_slice())
        };
        let thumb: Option<&[u8]> = message.thumb.as_deref();
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO messages (
                id, chat_id, sender, sender_name, from_me, ts, kind, body, status,
                view_once, played, payload, thumb, mime, file_name, file_size, duration,
                quote_id, quote_name, quote_text, edited, deleted, expire_ts, reaction,
                animated
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                       ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            params![
                message.id,
                message.chat_id,
                message.sender,
                message.sender_name,
                message.from_me as i64,
                message.ts,
                message.kind.as_str(),
                message.text,
                message.status.as_str(),
                message.view_once as i64,
                message.played as i64,
                payload,
                thumb,
                message.mime,
                message.file_name,
                message.file_size as i64,
                message.duration as i64,
                message.quote.as_ref().map(|q| q.id.clone()).unwrap_or_default(),
                message.quote.as_ref().map(|q| q.sender.clone()).unwrap_or_default(),
                message.quote.as_ref().map(|q| q.text.clone()).unwrap_or_default(),
                message.edited as i64,
                message.deleted as i64,
                message.expire_ts,
                message.reaction,
                message.animated as i64,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn insert_messages(&mut self, messages: &[Message]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut inserted = 0usize;
        for message in messages {
            let payload: Option<&[u8]> = if message.payload.is_empty() {
                None
            } else {
                Some(message.payload.as_slice())
            };
            let changed = tx.execute(
                "INSERT OR IGNORE INTO messages (
                    id, chat_id, sender, sender_name, from_me, ts, kind, body, status,
                    view_once, played, payload, thumb, mime, file_name, file_size, duration,
                    quote_id, quote_name, quote_text, edited, deleted, expire_ts, reaction,
                    animated
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                           ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                params![
                    message.id,
                    message.chat_id,
                    message.sender,
                    message.sender_name,
                    message.from_me as i64,
                    message.ts,
                    message.kind.as_str(),
                    message.text,
                    message.status.as_str(),
                    message.view_once as i64,
                    message.played as i64,
                    payload,
                    message.thumb.as_deref(),
                    message.mime,
                    message.file_name,
                    message.file_size as i64,
                    message.duration as i64,
                    message.quote.as_ref().map(|q| q.id.clone()).unwrap_or_default(),
                    message.quote.as_ref().map(|q| q.sender.clone()).unwrap_or_default(),
                    message.quote.as_ref().map(|q| q.text.clone()).unwrap_or_default(),
                    message.edited as i64,
                    message.deleted as i64,
                    message.expire_ts,
                    message.reaction,
                    message.animated as i64,
                ],
            )?;
            if changed > 0 {
                inserted += 1;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Invii rimasti in sospeso da un avvio precedente: senza una risposta del
    /// server non partiranno mai, quindi diventano "falliti" e si possono
    /// riprovare. `before` è il limite di tempo (timestamp in secondi).
    pub fn fail_stale_pending(&self, before: i64) -> usize {
        self.conn
            .execute(
                "UPDATE messages SET status = 'failed'
                  WHERE from_me = 1 AND status = 'pending' AND ts < ?1",
                params![before],
            )
            .unwrap_or(0)
    }

    /// Riapre un invio fallito: lo stato può tornare a `pending` solo da qui.
    pub fn retry_message(&self, chat_id: &str, message_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET status = 'pending'
              WHERE chat_id = ?1 AND id = ?2 AND status = 'failed'",
            params![chat_id, message_id],
        )?;
        Ok(())
    }

    pub fn update_status(&self, chat_id: &str, message_id: &str, status: Status) -> Result<()> {
        // Mai tornare indietro nello stato (sent -> delivered -> read).
        self.conn.execute(
            "UPDATE messages SET status = ?3
             WHERE chat_id = ?1 AND id = ?2
               AND CASE ?3
                     WHEN 'pending' THEN 0
                     WHEN 'sent' THEN 1
                     WHEN 'delivered' THEN 2
                     WHEN 'read' THEN 3
                     WHEN 'played' THEN 4
                     WHEN 'failed' THEN 5
                   END
                 > CASE status
                     WHEN 'pending' THEN 0
                     WHEN 'sent' THEN 1
                     WHEN 'delivered' THEN 2
                     WHEN 'read' THEN 3
                     WHEN 'played' THEN 4
                     WHEN 'failed' THEN 5
                   END",
            params![chat_id, message_id, status.as_str()],
        )?;
        Ok(())
    }

    pub fn rename_message_id(&self, chat_id: &str, old_id: &str, new_id: &str, ts: i64) -> Result<()> {
        if old_id == new_id {
            // L'id l'abbiamo scelto noi: la riga è già quella giusta, va solo
            // confermata (lo stato passa a "inviato").
            self.conn.execute(
                "UPDATE messages SET ts = ?3, status = 'sent'
                  WHERE chat_id = ?1 AND id = ?2",
                params![chat_id, old_id, ts],
            )?;
            return Ok(());
        }
        // Se l'eco del server è già arrivata esiste una riga con `new_id`:
        // quella è la verità (payload, stato, miniatura), quindi la copia
        // locale va rimossa invece di restare lì come duplicato.
        self.conn
            .execute(
                "DELETE FROM messages WHERE chat_id = ?1 AND id = ?2
                   AND EXISTS (SELECT 1 FROM messages WHERE chat_id = ?1 AND id = ?3)",
                params![chat_id, old_id, new_id],
            )
            .ok();
        self.conn
            .execute(
                "UPDATE OR IGNORE messages SET id = ?3, ts = ?4, status = 'sent'
                  WHERE chat_id = ?1 AND id = ?2",
                params![chat_id, old_id, new_id, ts],
            )
            .ok();
        Ok(())
    }

    /// Id di una bolla locale ancora in attesa che corrisponda all'eco del
    /// server (stesso testo, stesso minuto): evita il doppione quando il
    /// server rimanda indietro il messaggio appena inviato.
    pub fn find_pending(&self, chat_id: &str, body: &str, ts: i64) -> Option<String> {
        self.conn
            .query_row(
                "SELECT id FROM messages
                  WHERE chat_id = ?1 AND body = ?2 AND from_me = 1
                    AND status IN ('pending', 'sent', 'delivered')
                    AND id LIKE 'local-%'
                    AND ABS(ts - ?3) <= 300
                  ORDER BY ABS(ts - ?3) LIMIT 1",
                params![chat_id, body, ts],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Contenuto "una volta sola" consumato: non deve più essere leggibile
    /// dopo un riavvio.
    pub fn consume_once(&self, chat_id: &str, message_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET played = 1, body = '', payload = NULL, thumb = NULL
              WHERE chat_id = ?1 AND id = ?2",
            params![chat_id, message_id],
        )?;
        Ok(())
    }

    pub fn set_played(&self, chat_id: &str, message_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET played = 1 WHERE chat_id = ?1 AND id = ?2",
            params![chat_id, message_id],
        )?;
        Ok(())
    }

    pub fn apply_edit(&self, chat_id: &str, message_id: &str, text: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET body = ?3, edited = 1, ts = ts WHERE chat_id = ?1 AND id = ?2",
            params![chat_id, message_id, text],
        )?;
        let _ = ts;
        Ok(())
    }

    pub fn apply_revoke(&self, chat_id: &str, message_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET deleted = 1, body = '', payload = NULL WHERE chat_id = ?1 AND id = ?2",
            params![chat_id, message_id],
        )?;
        Ok(())
    }

    pub fn set_reaction(&self, chat_id: &str, message_id: &str, emoji: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET reaction = ?3 WHERE chat_id = ?1 AND id = ?2",
            params![chat_id, message_id, emoji],
        )?;
        Ok(())
    }

    pub fn mark_read(&self, chat_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chats SET unread = 0 WHERE id = ?1",
            params![chat_id],
        )?;
        Ok(())
    }

    pub fn bump_unread(&self, chat_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chats SET unread = unread + 1 WHERE id = ?1",
            params![chat_id],
        )?;
        Ok(())
    }

    pub fn touch_chat(&self, chat_id: &str, ts: i64, preview: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chats SET last_ts = MAX(last_ts, ?2), preview = ?3 WHERE id = ?1",
            params![chat_id, ts, preview],
        )?;
        Ok(())
    }

    pub fn delete_expired(&self, now: i64) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chat_id, id FROM messages WHERE expire_ts > 0 AND expire_ts <= ?1")?;
        let rows = stmt.query_map(params![now], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut doomed = Vec::new();
        for row in rows {
            doomed.push(row?);
        }
        drop(stmt);
        for (chat, id) in &doomed {
            self.conn.execute(
                "DELETE FROM messages WHERE chat_id = ?1 AND id = ?2",
                params![chat, id],
            )?;
        }
        Ok(doomed.into_iter().map(|(_, id)| id).collect())
    }

    pub fn is_known_chat(&self, chat_id: &str) -> Result<bool> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM chats WHERE id = ?1",
                params![chat_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    pub fn wipe(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "DELETE FROM messages; DELETE FROM chats; DELETE FROM contacts; DELETE FROM aliases;",
            )
            .context("svuotamento archivio")?;
        Ok(())
    }

    pub fn unread_ids(&self, chat_id: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sender FROM messages
             WHERE chat_id = ?1 AND from_me = 0 ORDER BY ts DESC LIMIT 40",
        )?;
        let rows = stmt.query_map(params![chat_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

impl Store {
    pub fn load_contacts(&self) -> Result<Vec<Contact>> {
        let mut stmt = self
            .conn
            .prepare("SELECT jid, name, name_rank, phone, avatar FROM contacts")?;
        let rows = stmt.query_map([], |row| {
            Ok(Contact {
                jid: row.get(0)?,
                name: row.get(1)?,
                rank: row.get::<_, i64>(2)?.max(0) as u8,
                phone: row.get(3)?,
                avatar: row.get::<_, Option<Vec<u8>>>(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn save_contact(&self, contact: &Contact, stamp: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO contacts (jid, name, name_rank, phone, avatar, avatar_ts, checked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
             ON CONFLICT(jid) DO UPDATE SET
                name = CASE WHEN ?3 >= contacts.name_rank AND ?2 <> ''
                            THEN ?2 ELSE contacts.name END,
                name_rank = MAX(contacts.name_rank, ?3),
                phone = CASE WHEN ?4 <> '' THEN ?4 ELSE contacts.phone END,
                avatar = CASE WHEN ?6 >= contacts.avatar_ts THEN ?5 ELSE contacts.avatar END,
                avatar_ts = MAX(contacts.avatar_ts, ?6),
                checked = 1",
            params![
                contact.jid,
                contact.name,
                contact.rank as i64,
                contact.phone,
                contact.avatar,
                stamp,
            ],
        )?;
        Ok(())
    }

    pub fn set_contact_name(&self, jid: &str, name: &str, rank: u8) -> Result<()> {
        if jid.is_empty() || name.trim().is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO contacts (jid, name, name_rank, phone)
             VALUES (?1, ?2, ?3, '')
             ON CONFLICT(jid) DO UPDATE SET
                name = CASE WHEN ?3 >= contacts.name_rank THEN ?2 ELSE contacts.name END,
                name_rank = MAX(contacts.name_rank, ?3)",
            params![jid, name.trim(), rank as i64],
        )?;
        Ok(())
    }

    pub fn set_contact_phone(&self, jid: &str, phone: &str) -> Result<()> {
        if jid.is_empty() || phone.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO contacts (jid, phone) VALUES (?1, ?2)
             ON CONFLICT(jid) DO UPDATE SET phone = ?2",
            params![jid, phone],
        )?;
        Ok(())
    }

    pub fn save_alias(&self, lid: &str, pn: &str) -> Result<()> {
        if lid.is_empty() || pn.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO aliases (lid, pn) VALUES (?1, ?2)
             ON CONFLICT(lid) DO UPDATE SET pn = ?2",
            params![lid, pn],
        )?;
        Ok(())
    }

    pub fn load_aliases(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT lid, pn FROM aliases")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Recupera i nomi già visti nei messaggi archiviati: è la sincronizzazione
    /// più economica che esista (una query, nessuna rete) e riporta i nomi
    /// anche dopo un riavvio o una sessione senza storico.
    pub fn backfill_names(&self, aliases: &HashMap<String, String>) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT sender, sender_name, COUNT(*) AS n
               FROM messages
              WHERE from_me = 0 AND sender_name <> ''
              GROUP BY sender, sender_name
              ORDER BY n DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
            ))
        })?;
        let mut seen: HashMap<String, String> = HashMap::new();
        for row in rows {
            let (sender, name) = row?;
            let name = name.trim().to_string();
            if sender.is_empty() || name.is_empty() || crate::util::jid_is_group(&sender) {
                continue;
            }
            let jid = aliases.get(&sender).cloned().unwrap_or(sender);
            seen.entry(jid).or_insert(name);
        }
        let mut learned = 0usize;
        for (jid, name) in seen {
            if crate::util::looks_like_phone(&name) {
                if self.set_contact_phone(&jid, name.trim_start_matches('+')).is_ok() {
                    learned += 1;
                }
            } else if self.set_contact_name(&jid, &name, 1).is_ok() {
                learned += 1;
            }
        }
        Ok(learned)
    }

    /// Salva la foto profilo. `bytes` vuoto = nessuna foto disponibile: la
    /// riga resta con `checked = 1` per non ritentare a ogni avvio.
    pub fn set_contact_avatar(&self, jid: &str, bytes: Option<&[u8]>, stamp: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO contacts (jid, avatar, avatar_ts, checked) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(jid) DO UPDATE SET avatar = ?2, avatar_ts = ?3, checked = 1",
            params![jid, bytes, stamp],
        )?;
        Ok(())
    }

    pub fn contact_checked(&self, jid: &str) -> bool {
        self.conn
            .query_row(
                "SELECT checked FROM contacts WHERE jid = ?1",
                params![jid],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0)
            != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let path = std::env::temp_dir().join(format!(
            "whats-open-test-{name}-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Store::open(&path).expect("apertura archivio di prova")
    }

    fn message(chat: &str, id: &str, ts: i64, text: &str) -> Message {
        Message {
            id: id.to_string(),
            chat_id: chat.to_string(),
            sender: "393330000000@s.whatsapp.net".to_string(),
            sender_name: "Prova".to_string(),
            ts,
            kind: Kind::Text,
            text: text.to_string(),
            status: Status::Sent,
            ..Default::default()
        }
    }

    #[test]
    fn messaggi_restano_in_ordine_cronologico() {
        let mut store = temp_store("ordine");
        store
            .insert_messages(&[
                message("a@s.whatsapp.net", "2", 200, "secondo"),
                message("a@s.whatsapp.net", "1", 100, "primo"),
                message("a@s.whatsapp.net", "3", 300, "terzo"),
            ])
            .unwrap();

        let loaded = store.load_messages("a@s.whatsapp.net", 50).unwrap();
        let bodies: Vec<&str> = loaded.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(bodies, ["primo", "secondo", "terzo"]);
    }

    #[test]
    fn inserimento_ripetuto_non_duplica() {
        let store = temp_store("duplicati");
        let msg = message("b@s.whatsapp.net", "x1", 10, "ciao");
        assert!(store.insert_message(&msg).unwrap());
        assert!(!store.insert_message(&msg).unwrap());
        assert_eq!(store.load_messages("b@s.whatsapp.net", 10).unwrap().len(), 1);
    }

    #[test]
    fn lo_stato_non_torna_indietro() {
        let store = temp_store("stato");
        let msg = message("c@s.whatsapp.net", "s1", 5, "ciao");
        store.insert_message(&msg).unwrap();
        store
            .update_status("c@s.whatsapp.net", "s1", Status::Read)
            .unwrap();
        store
            .update_status("c@s.whatsapp.net", "s1", Status::Sent)
            .unwrap();
        let loaded = store.load_messages("c@s.whatsapp.net", 10).unwrap();
        assert_eq!(loaded[0].status, Status::Read);
    }

    #[test]
    fn modifica_e_revoca_aggiornano_la_riga() {
        let store = temp_store("modifiche");
        store
            .insert_message(&message("d@s.whatsapp.net", "e1", 7, "vecchio"))
            .unwrap();
        store
            .apply_edit("d@s.whatsapp.net", "e1", "nuovo", 8)
            .unwrap();
        let edited = store.load_messages("d@s.whatsapp.net", 10).unwrap();
        assert_eq!(edited[0].text, "nuovo");
        assert!(edited[0].edited);
        assert!(!edited[0].deleted);

        store.apply_revoke("d@s.whatsapp.net", "e1").unwrap();
        let revoked = store.load_messages("d@s.whatsapp.net", 10).unwrap();
        assert!(revoked[0].deleted);
        assert_eq!(revoked[0].body(), "Questo messaggio è stato eliminato");
    }

    #[test]
    fn i_messaggi_scaduti_vengono_eliminati() {
        let store = temp_store("scadenza");
        let mut msg = message("e@s.whatsapp.net", "v1", 10, "effimero");
        msg.expire_ts = 20;
        store.insert_message(&msg).unwrap();

        let removed = store.delete_expired(30).unwrap();
        assert_eq!(removed, vec!["v1".to_string()]);
        assert!(store.load_messages("e@s.whatsapp.net", 10).unwrap().is_empty());
    }

    #[test]
    fn gli_invii_rimasti_in_sospeso_diventano_falliti() {
        let store = temp_store("sospesi");
        let mut pending = message("i@s.whatsapp.net", "p1", 100, "in attesa");
        pending.from_me = true;
        pending.status = Status::Pending;
        store.insert_message(&pending).unwrap();
        let mut fresh = message("i@s.whatsapp.net", "p2", 500, "nuovo");
        fresh.from_me = true;
        fresh.status = Status::Pending;
        store.insert_message(&fresh).unwrap();

        assert_eq!(store.fail_stale_pending(200), 1);
        let loaded = store.load_messages("i@s.whatsapp.net", 10).unwrap();
        assert_eq!(loaded[0].status, Status::Failed);
        assert_eq!(loaded[1].status, Status::Pending);
    }

    #[test]
    fn i_messaggi_seguono_l_alias_anche_senza_chat() {
        let store = temp_store("alias-messaggi");
        store
            .insert_message(&message("111@lid", "m1", 10, "ciao"))
            .unwrap();
        store
            .rename_jid("111@lid", "393330000000@s.whatsapp.net")
            .unwrap();
        let loaded = store.load_messages("393330000000@s.whatsapp.net", 10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(store.load_messages("111@lid", 10).unwrap().is_empty());
    }

    #[test]
    fn gli_alias_sopravvivono_al_riavvio() {
        let store = temp_store("alias");
        store.save_alias("111@lid", "393330000000@s.whatsapp.net").unwrap();
        let loaded = store.load_aliases().unwrap();
        assert_eq!(
            loaded,
            vec![(
                "111@lid".to_string(),
                "393330000000@s.whatsapp.net".to_string()
            )]
        );
    }

    #[test]
    fn i_nomi_si_recuperano_dai_messaggi() {
        let mut store = temp_store("nomi");
        let mut first = message("393330000000@s.whatsapp.net", "n1", 10, "ciao");
        first.sender_name = "Mamma".to_string();
        let mut second = message("393330000000@s.whatsapp.net", "n2", 11, "di nuovo");
        second.sender_name = "Mamma".to_string();
        store.insert_messages(&[first, second]).unwrap();

        let mut aliases = HashMap::new();
        aliases.insert(
            "111@lid".to_string(),
            "393330000000@s.whatsapp.net".to_string(),
        );
        store.backfill_names(&aliases).unwrap();

        let contacts = store.load_contacts().unwrap();
        let contact = contacts
            .iter()
            .find(|contact| contact.jid == "393330000000@s.whatsapp.net")
            .expect("contatto creato dai messaggi");
        assert_eq!(contact.name, "Mamma");
    }

    #[test]
    fn stesso_id_non_cancella_il_messaggio() {
        let store = temp_store("stesso-id");
        store
            .insert_message(&message("g@s.whatsapp.net", "ABC123", 11, "in uscita"))
            .unwrap();
        // L'id scelto in locale è già quello del server: la conferma non deve
        // toccare la riga (prima la cancellava come doppione).
        store
            .rename_message_id("g@s.whatsapp.net", "ABC123", "ABC123", 12)
            .unwrap();
        let loaded = store.load_messages("g@s.whatsapp.net", 10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "ABC123");
        assert_eq!(loaded[0].status, Status::Sent);
    }

    #[test]
    fn contenuto_una_volta_non_resta_leggibile() {
        let store = temp_store("una-volta");
        let mut msg = message("h@s.whatsapp.net", "once1", 10, "segreto");
        msg.view_once = true;
        store.insert_message(&msg).unwrap();
        store.consume_once("h@s.whatsapp.net", "once1").unwrap();
        let loaded = store.load_messages("h@s.whatsapp.net", 10).unwrap();
        assert!(loaded[0].played);
        assert!(loaded[0].text.is_empty());
    }

    #[test]
    fn rinominare_l_id_di_invio_conserva_il_messaggio() {
        let store = temp_store("rinomina");
        store
            .insert_message(&message("f@s.whatsapp.net", "local-1", 11, "in uscita"))
            .unwrap();
        store
            .rename_message_id("f@s.whatsapp.net", "local-1", "SERVER1", 12)
            .unwrap();

        let loaded = store.load_messages("f@s.whatsapp.net", 10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "SERVER1");
        assert_eq!(loaded[0].ts, 12);
    }
}
