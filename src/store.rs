//! Archivio locale (SQLite) di chat e messaggi.
//!
//! Il database della sessione (credenziali, chiavi) è separato: lo gestisce la
//! libreria del protocollo in `session.db`. Qui vivono solo i dati visibili.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{Chat, Kind, Message, Status};

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
                    reaction
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
                quote_id, quote_name, quote_text, edited, deleted, expire_ts, reaction
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                       ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
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
                    quote_id, quote_name, quote_text, edited, deleted, expire_ts, reaction
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                           ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
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
                ],
            )?;
            if changed > 0 {
                inserted += 1;
            }
        }
        tx.commit()?;
        Ok(inserted)
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
        self.conn
            .execute(
                "UPDATE OR IGNORE messages SET id = ?3, ts = ?4 WHERE chat_id = ?1 AND id = ?2",
                params![chat_id, old_id, new_id, ts],
            )
            .ok();
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
            .execute_batch("DELETE FROM messages; DELETE FROM chats;")
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
