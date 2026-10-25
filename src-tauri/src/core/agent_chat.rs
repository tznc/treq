//! Agent terminal chat logs, split from TUI output the same way AgentAPI does.
//!
//! Shell terminals are never registered. Persistence lives under
//! `.treq/agent-chats/{session_id}.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// A session's chat held in memory while it records. The file stamp says
/// which file contents `chat` mirrors: when the file's modification time and
/// length differ, someone else wrote it and it is read again.
struct CachedChat {
  modified: Option<SystemTime>,
  len: u64,
  chat: AgentChat,
  /// How many leading `chat.messages` the file already holds. Messages past
  /// it are appended to the file instead of writing the whole chat again.
  persisted: usize,
  /// The agent's last logged screen, so a later one is compared with it
  /// rather than with every message since the user spoke. `None` after a load.
  snapshot: Option<String>,
}

static CHAT_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedChat>>> = OnceLock::new();

/// Forgets a finished session's in-memory chat and its lock. The file stays.
pub fn end_session(repo_path: &str, session_id: i64) {
  let path = chat_path(repo_path, session_id);
  if let Some(cache) = CHAT_CACHE.get() {
    cache
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
      .remove(&path);
  }
  chat_locks()
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .remove(&path);
}

/// One lock per chat file, so a session's once-a-second screen recording never
/// waits on another session's I/O.
type ChatLocks = Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>;

fn chat_locks() -> &'static ChatLocks {
  static LOCKS: OnceLock<ChatLocks> = OnceLock::new();
  LOCKS.get_or_init(Mutex::default)
}

fn chat_lock(repo_path: &str, session_id: i64) -> Arc<Mutex<()>> {
  let mut locks = chat_locks()
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  locks
    .entry(chat_path(repo_path, session_id))
    .or_default()
    .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
  Claude,
  Codex,
  Cursor,
  Copilot,
}

impl AgentType {
  pub fn parse(value: &str) -> Self {
    match value {
      "codex" => Self::Codex,
      "cursor" => Self::Cursor,
      "copilot" => Self::Copilot,
      _ => Self::Claude,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
  User,
  Agent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
  pub id: usize,
  pub role: ChatRole,
  pub message: String,
  pub time: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChat {
  pub session_id: i64,
  pub pty_session_id: String,
  pub name: String,
  pub agent: AgentType,
  pub workspace_id: Option<i64>,
  pub created_at: String,
  #[serde(default)]
  pub screen_before_last_user_message: String,
  pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatSummary {
  pub session_id: i64,
  pub pty_session_id: String,
  pub name: String,
  pub agent: String,
  pub workspace_id: Option<i64>,
  pub created_at: String,
  pub message_count: usize,
}

pub fn agent_chats_dir(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq").join("agent-chats")
}

fn chat_path(repo_path: &str, session_id: i64) -> PathBuf {
  agent_chats_dir(repo_path).join(format!("{session_id}.json"))
}

fn now_rfc3339() -> String {
  chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn reindex(messages: &mut [ChatMessage]) {
  for (i, message) in messages.iter_mut().enumerate() {
    message.id = i;
  }
}

const WHITE_SPACE: &str = " \t\n\r\u{000c}\u{000b}";

fn trim_whitespace(msg: &str) -> String {
  msg
    .trim_matches(|c: char| WHITE_SPACE.contains(c))
    .to_string()
}

fn index_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
  if needle.is_empty() {
    return Some(0);
  }
  if needle.len() > haystack.len() {
    return None;
  }
  haystack
    .windows(needle.len())
    .position(|window| window == needle)
}

fn normalize_and_get_rune_line_mapping(msg_raw: &str) -> (Vec<char>, Vec<String>, Vec<usize>) {
  let msg_lines: Vec<String> = msg_raw.split('\n').map(str::to_string).collect();
  let mut runes = Vec::new();
  let mut locations = Vec::new();
  for (line_idx, line) in msg_lines.iter().enumerate() {
    for ch in line.chars() {
      if !WHITE_SPACE.contains(ch) {
        runes.push(ch);
        locations.push(line_idx);
      }
    }
  }
  (runes, msg_lines, locations)
}

fn find_user_input_start_idx(
  msg: &[char],
  msg_rune_line_locations: &[usize],
  user_input: &[char],
  user_input_line_locations: &[usize],
) -> Option<usize> {
  let mut user_input_prefix_len = 0usize;
  for (i, line_idx) in user_input_line_locations.iter().enumerate() {
    if *line_idx > 0 || i >= 6 {
      break;
    }
    user_input_prefix_len = i + 1;
  }
  if user_input_prefix_len == 0 {
    return None;
  }
  let user_input_prefix = &user_input[..user_input_prefix_len];

  let mut msg_prefix_len = 0usize;
  for (i, line_idx) in msg_rune_line_locations.iter().enumerate() {
    if *line_idx > 5 {
      break;
    }
    msg_prefix_len = i + 1;
  }
  if msg_prefix_len < 25 {
    msg_prefix_len = 25;
  }
  if msg_prefix_len > msg.len() {
    msg_prefix_len = msg.len();
  }
  index_subslice(&msg[..msg_prefix_len], user_input_prefix)
}

fn find_next_match(
  known_msg_match_idx: usize,
  known_user_input_match_idx: usize,
  msg: &[char],
  user_input: &[char],
) -> Option<(usize, usize)> {
  for i in 0..5 {
    for j in 0..5 {
      let user_input_idx = known_user_input_match_idx + i + 1;
      let msg_idx = known_msg_match_idx + j + 1;
      if user_input_idx >= user_input.len() || msg_idx >= msg.len() {
        return None;
      }
      if user_input[user_input_idx] == msg[msg_idx] {
        return Some((msg_idx, user_input_idx));
      }
    }
  }
  None
}

fn find_user_input_end_idx(
  user_input_start_idx: usize,
  msg: &[char],
  user_input: &[char],
) -> usize {
  let mut user_input_idx = 0usize;
  let mut msg_idx = user_input_start_idx;
  while let Some((m, u)) = find_next_match(msg_idx, user_input_idx, msg, user_input) {
    msg_idx = m;
    user_input_idx = u;
  }
  msg_idx
}

fn skip_trailing_input_box_line(
  lines: &[String],
  current_idx: usize,
  markers: &[&str],
) -> Option<usize> {
  let next = current_idx + 1;
  if next >= lines.len() {
    return None;
  }
  let line = &lines[next];
  if markers.iter().all(|marker| line.contains(marker)) {
    Some(next)
  } else {
    None
  }
}

/// Strip echoed user input from the start of an agent TUI snapshot.
pub fn remove_user_input(msg_raw: &str, user_input_raw: &str, agent: AgentType) -> String {
  if user_input_raw.is_empty() {
    return msg_raw.to_string();
  }
  let (msg, msg_lines, msg_rune_line_locations) = normalize_and_get_rune_line_mapping(msg_raw);
  let (user_input, _, user_input_line_locations) =
    normalize_and_get_rune_line_mapping(user_input_raw);
  let Some(start) = find_user_input_start_idx(
    &msg,
    &msg_rune_line_locations,
    &user_input,
    &user_input_line_locations,
  ) else {
    return msg_raw.to_string();
  };
  let end = find_user_input_end_idx(start, &msg, &user_input);
  let mut last_line = msg_rune_line_locations[end];
  if agent == AgentType::Cursor {
    if let Some(idx) = skip_trailing_input_box_line(&msg_lines, last_line, &["┘", "└"]) {
      last_line = idx;
    }
  }
  msg_lines[last_line + 1..].join("\n")
}

fn contains_horizontal_border(line: &str) -> bool {
  line.contains("───────────────") || line.contains("╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌")
}

fn find_greater_than_message_box(lines: &[String]) -> Option<usize> {
  let start = lines.len().saturating_sub(6);
  for i in (start..lines.len()).rev() {
    if lines[i].contains('>') {
      if i > 0 && contains_horizontal_border(&lines[i - 1]) {
        return Some(i - 1);
      }
      return Some(i);
    }
  }
  None
}

fn find_generic_slim_message_box(lines: &[String]) -> Option<usize> {
  if lines.len() < 3 {
    return None;
  }
  let start = lines.len().saturating_sub(9);
  let end = lines.len() - 3;
  (start..=end).rev().find(|&i| {
    contains_horizontal_border(&lines[i])
      && (lines[i + 1].contains('|') || lines[i + 1].contains('│') || lines[i + 1].contains('❯'))
      && contains_horizontal_border(&lines[i + 2])
  })
}

fn remove_message_box(msg: &str) -> String {
  let mut lines: Vec<String> = msg.split('\n').map(str::to_string).collect();
  let start =
    find_greater_than_message_box(&lines).or_else(|| find_generic_slim_message_box(&lines));
  if let Some(idx) = start {
    lines.truncate(idx);
  }
  lines.join("\n")
}

fn remove_codex_message_box(msg: &str) -> String {
  let mut lines: Vec<String> = msg.split('\n').map(str::to_string).collect();
  if lines.len() >= 3 && lines[lines.len() - 3].contains('›') {
    let idx = lines.len() - 3;
    let keep = lines[idx + 2].clone();
    lines.truncate(idx);
    lines.push(keep);
  }
  lines.join("\n")
}

fn trim_empty_lines(message: &str) -> String {
  let lines: Vec<&str> = message.split('\n').collect();
  let first = lines
    .iter()
    .position(|line| !line.trim().is_empty())
    .unwrap_or(lines.len());
  let last = lines
    .iter()
    .rposition(|line| !line.trim().is_empty())
    .map(|i| i + 1)
    .unwrap_or(0);
  if first >= last {
    return String::new();
  }
  lines[first..last].join("\n")
}

/// Remove echoed user input and the trailing TUI input box from an agent snapshot.
pub fn format_agent_message(agent: AgentType, message: &str, user_input: &str) -> String {
  let without_input = remove_user_input(message, user_input, agent);
  let without_box = match agent {
    AgentType::Codex | AgentType::Copilot => remove_codex_message_box(&without_input),
    _ => remove_message_box(&without_input),
  };
  trim_empty_lines(&without_box)
}

/// Diff two terminal snapshots; new text below shared prefix is the next agent message.
pub fn screen_diff(old_screen: &str, new_screen: &str) -> String {
  let new_lines: Vec<&str> = new_screen.split('\n').collect();
  let old_set: std::collections::HashSet<&str> = old_screen.split('\n').collect();
  let first_non_matching = new_lines
    .iter()
    .position(|line| !old_set.contains(line))
    .unwrap_or(new_lines.len());
  let new_section = &new_lines[first_non_matching..];
  trim_empty_lines(&new_section.join("\n"))
}

fn last_user_text(messages: &[ChatMessage]) -> &str {
  messages
    .iter()
    .rev()
    .find(|m| m.role == ChatRole::User)
    .map(|m| m.message.as_str())
    .unwrap_or_default()
}

/// What the agent's trailing messages add up to: the screen last logged.
fn trailing_agent_text(messages: &[ChatMessage]) -> String {
  messages
    .iter()
    .rev()
    .take_while(|m| m.role == ChatRole::Agent)
    .map(|m| m.message.as_str())
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect()
}

/// The lines of `current` that `previous` did not already show, or `None`
/// when nothing new is left. Snapshots are cumulative until the screen
/// scrolls, so the text after `previous` is new; once it scrolled, the new
/// lines are the ones after the lines both snapshots share. A line the agent
/// redraws in place (a spinner) is the only one allowed to differ.
fn new_portion(previous: &str, current: &str) -> Option<String> {
  if let Some(rest) = current.strip_prefix(previous) {
    return Some(trim_whitespace(rest)).filter(|rest| !rest.is_empty());
  }
  let before: Vec<&str> = previous.split('\n').collect();
  let now: Vec<&str> = current.split('\n').collect();
  for skip in 0..before.len() {
    for tail in [&before[skip..], &before[skip..before.len() - 1]] {
      let shared = tail.len();
      if tail.iter().any(|line| !line.trim().is_empty()) && now.get(..shared) == Some(tail) {
        return Some(trim_empty_lines(&now[shared..].join("\n"))).filter(|rest| !rest.is_empty());
      }
    }
  }
  Some(current.to_string())
}

/// Returns whether the chat changed.
fn update_last_agent_message(
  chat: &mut AgentChat,
  snapshot: &mut Option<String>,
  screen: &str,
  timestamp: &str,
) -> bool {
  let raw = screen_diff(&chat.screen_before_last_user_message, screen);
  let agent_message = format_agent_message(chat.agent, &raw, last_user_text(&chat.messages));
  let last_is_user = chat
    .messages
    .last()
    .map(|m| m.role == ChatRole::User)
    .unwrap_or(true);
  let previous = snapshot.get_or_insert_with(|| {
    if last_is_user {
      String::new()
    } else {
      trailing_agent_text(&chat.messages)
    }
  });
  if *previous == agent_message || agent_message.is_empty() {
    return false;
  }
  // Terminal snapshots are cumulative. Persist only the newly arrived portion
  // so every screen update remains an individually timestamped log event.
  let event_message = if last_is_user {
    Some(agent_message.clone())
  } else {
    new_portion(previous, &agent_message)
  };
  *previous = agent_message;
  let Some(event_message) = event_message else {
    return false;
  };
  let conversation_message = ChatMessage {
    id: 0,
    role: ChatRole::Agent,
    message: event_message,
    time: timestamp.to_string(),
  };
  if chat
    .messages
    .last()
    .is_some_and(|message| message.role == ChatRole::Agent && message.message.is_empty())
  {
    let last = chat.messages.len() - 1;
    chat.messages[last] = conversation_message;
  } else {
    chat.messages.push(conversation_message);
  }
  reindex(&mut chat.messages);
  true
}

/// Record a user message. `screen_before` is the terminal snapshot taken before keystrokes.
pub fn note_user_message(
  chat: &mut AgentChat,
  snapshot: &mut Option<String>,
  screen_before: &str,
  text: &str,
  timestamp: &str,
) {
  let trimmed = trim_whitespace(text);
  if trimmed.is_empty() {
    return;
  }
  update_last_agent_message(chat, snapshot, screen_before, timestamp);
  chat.screen_before_last_user_message = screen_before.to_string();
  chat.messages.push(ChatMessage {
    id: 0,
    role: ChatRole::User,
    message: trimmed,
    time: timestamp.to_string(),
  });
  reindex(&mut chat.messages);
  // The agent's next screen is compared with nothing: it is a new reply.
  *snapshot = Some(String::new());
}

/// Record a later screen snapshot; updates (or appends) the current agent message.
/// Returns whether the chat changed.
pub fn note_screen(
  chat: &mut AgentChat,
  snapshot: &mut Option<String>,
  screen: &str,
  timestamp: &str,
) -> bool {
  update_last_agent_message(chat, snapshot, screen, timestamp)
}

fn stamped(metadata: &fs::Metadata, chat: AgentChat, persisted: usize) -> CachedChat {
  CachedChat {
    modified: metadata.modified().ok(),
    len: metadata.len(),
    chat,
    persisted,
    snapshot: None,
  }
}

/// The session's chat: the one held in memory while the file is as this
/// process left it, otherwise the file. The caller holds the session's lock
/// and puts the chat back with [`keep`].
fn take_chat_unlocked(repo_path: &str, session_id: i64) -> Result<Option<CachedChat>, String> {
  let path = chat_path(repo_path, session_id);
  let Ok(metadata) = fs::metadata(&path) else {
    return Ok(None);
  };
  let cache = CHAT_CACHE.get_or_init(Mutex::default);
  let mut cache = cache
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  if let Some(cached) = cache.remove(&path) {
    if cached.modified == metadata.modified().ok() && cached.len == metadata.len() {
      return Ok(Some(cached));
    }
  }
  drop(cache);
  let raw = fs::read_to_string(&path).map_err(|e| format!("Failed to read agent chat: {e}"))?;
  let chat: AgentChat =
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse agent chat: {e}"))?;
  let persisted = chat.messages.len();
  Ok(Some(stamped(&metadata, chat, persisted)))
}

fn keep(repo_path: &str, session_id: i64, cached: CachedChat) {
  CHAT_CACHE
    .get_or_init(Mutex::default)
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .insert(chat_path(repo_path, session_id), cached);
}

fn load_chat_unlocked(repo_path: &str, session_id: i64) -> Result<Option<AgentChat>, String> {
  let Some(cached) = take_chat_unlocked(repo_path, session_id)? else {
    return Ok(None);
  };
  let chat = cached.chat.clone();
  keep(repo_path, session_id, cached);
  Ok(Some(chat))
}

/// Writes the whole chat, replacing the file atomically so a reader never
/// sees a half-written one.
fn save_chat_unlocked(repo_path: &str, cached: &mut CachedChat) -> Result<(), String> {
  let dir = agent_chats_dir(repo_path);
  fs::create_dir_all(&dir).map_err(|e| format!("Failed to create agent-chats dir: {e}"))?;
  let path = chat_path(repo_path, cached.chat.session_id);
  let raw = serde_json::to_string(&cached.chat)
    .map_err(|e| format!("Failed to serialize agent chat: {e}"))?;
  let tmp = path.with_extension("json.tmp");
  fs::write(&tmp, raw).map_err(|e| format!("Failed to write agent chat: {e}"))?;
  fs::rename(&tmp, &path).map_err(|e| format!("Failed to write agent chat: {e}"))?;
  restamp(&path, cached);
  Ok(())
}

fn restamp(path: &Path, cached: &mut CachedChat) {
  cached.persisted = cached.chat.messages.len();
  if let Ok(metadata) = fs::metadata(path) {
    cached.modified = metadata.modified().ok();
    cached.len = metadata.len();
  }
}

/// Adds the messages the file lacks to the end of its `messages` array,
/// writing only their bytes. The file is the compact serialization of the
/// chat, which ends `…}]}`.
fn append_chat_unlocked(repo_path: &str, cached: &mut CachedChat) -> Result<(), String> {
  use std::io::{Read, Seek, SeekFrom, Write};
  let new = &cached.chat.messages[cached.persisted..];
  if new.is_empty() {
    return Ok(());
  }
  if cached.persisted == 0 || cached.len < 2 {
    return save_chat_unlocked(repo_path, cached);
  }
  let mut bytes = Vec::new();
  for message in new {
    bytes.push(b',');
    serde_json::to_writer(&mut bytes, message)
      .map_err(|e| format!("Failed to serialize agent chat: {e}"))?;
  }
  bytes.extend_from_slice(b"]}");
  let path = chat_path(repo_path, cached.chat.session_id);
  // Only a file that ends as this process writes it can be appended to;
  // another writer's formatting is replaced by a whole write.
  let written = fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(&path)
    .and_then(|mut file| {
      let mut tail = [0u8; 2];
      file.seek(SeekFrom::Start(cached.len - 2))?;
      file.read_exact(&mut tail)?;
      if &tail != b"]}" {
        return Err(std::io::Error::other("unexpected end of chat file"));
      }
      file.seek(SeekFrom::Start(cached.len - 2))?;
      file.write_all(&bytes)
    });
  if written.is_err() {
    return save_chat_unlocked(repo_path, cached);
  }
  restamp(&path, cached);
  Ok(())
}

pub fn register_agent_chat(
  repo_path: &str,
  session_id: i64,
  pty_session_id: &str,
  name: &str,
  agent: &str,
  workspace_id: Option<i64>,
  initial_prompt: Option<&str>,
) -> Result<AgentChat, String> {
  let lock = chat_lock(repo_path, session_id);
  let _guard = lock.lock().map_err(|e| e.to_string())?;
  if let Some(existing) = load_chat_unlocked(repo_path, session_id)? {
    return Ok(existing);
  }
  let created_at = now_rfc3339();
  let mut messages = vec![ChatMessage {
    id: 0,
    role: ChatRole::Agent,
    message: String::new(),
    time: created_at.clone(),
  }];
  if let Some(prompt) = initial_prompt
    .map(str::trim)
    .filter(|prompt| !prompt.is_empty())
  {
    messages.push(ChatMessage {
      id: 1,
      role: ChatRole::User,
      message: prompt.to_string(),
      time: created_at.clone(),
    });
  }
  let chat = AgentChat {
    session_id,
    pty_session_id: pty_session_id.to_string(),
    name: name.to_string(),
    agent: AgentType::parse(agent),
    workspace_id,
    created_at,
    screen_before_last_user_message: String::new(),
    messages,
  };
  let mut cached = CachedChat {
    modified: None,
    len: 0,
    chat: chat.clone(),
    persisted: 0,
    snapshot: None,
  };
  save_chat_unlocked(repo_path, &mut cached)?;
  keep(repo_path, session_id, cached);
  Ok(chat)
}

fn registered(repo_path: &str, session_id: i64) -> Result<CachedChat, String> {
  take_chat_unlocked(repo_path, session_id)?
    .ok_or_else(|| format!("Agent chat {session_id} is not registered"))
}

pub fn record_user_message(
  repo_path: &str,
  session_id: i64,
  screen_before: &str,
  text: &str,
) -> Result<(), String> {
  let lock = chat_lock(repo_path, session_id);
  let _guard = lock.lock().map_err(|e| e.to_string())?;
  let mut cached = registered(repo_path, session_id)?;
  let CachedChat { chat, snapshot, .. } = &mut cached;
  note_user_message(chat, snapshot, screen_before, text, &now_rfc3339());
  // The message also moves `screen_before_last_user_message`, which comes
  // before the messages in the file: this is the one record that rewrites it.
  let saved = save_chat_unlocked(repo_path, &mut cached);
  keep(repo_path, session_id, cached);
  saved
}

pub fn record_screen(repo_path: &str, session_id: i64, screen: &str) -> Result<(), String> {
  let lock = chat_lock(repo_path, session_id);
  let _guard = lock.lock().map_err(|e| e.to_string())?;
  let mut cached = registered(repo_path, session_id)?;
  // The first reply takes the place of the empty message the chat starts with.
  let replaces = cached.persisted == cached.chat.messages.len()
    && cached
      .chat
      .messages
      .last()
      .is_some_and(|m| m.role == ChatRole::Agent && m.message.is_empty());
  let CachedChat { chat, snapshot, .. } = &mut cached;
  // Most snapshots repeat the last one; write nothing then.
  let saved = if note_screen(chat, snapshot, screen, &now_rfc3339()) {
    if replaces {
      save_chat_unlocked(repo_path, &mut cached)
    } else {
      append_chat_unlocked(repo_path, &mut cached)
    }
  } else {
    Ok(())
  };
  keep(repo_path, session_id, cached);
  saved
}

pub fn list_agent_chats(repo_path: &str) -> Result<Vec<AgentChatSummary>, String> {
  let dir = agent_chats_dir(repo_path);
  if !dir.exists() {
    return Ok(Vec::new());
  }
  let mut summaries = Vec::new();
  for entry in fs::read_dir(&dir).map_err(|e| format!("Failed to read agent-chats dir: {e}"))? {
    let entry = entry.map_err(|e| e.to_string())?;
    let path = entry.path();
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
      continue;
    }
    // Appends are not atomic, so read under the session's lock.
    let lock = path
      .file_stem()
      .and_then(|stem| stem.to_str())
      .and_then(|stem| stem.parse::<i64>().ok())
      .map(|session_id| chat_lock(repo_path, session_id));
    let _guard = lock.as_ref().map(|lock| lock.lock());
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let chat: AgentChat = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    summaries.push(AgentChatSummary {
      session_id: chat.session_id,
      pty_session_id: chat.pty_session_id,
      name: chat.name,
      agent: match chat.agent {
        AgentType::Claude => "claude".to_string(),
        AgentType::Codex => "codex".to_string(),
        AgentType::Cursor => "cursor".to_string(),
        AgentType::Copilot => "copilot".to_string(),
      },
      workspace_id: chat.workspace_id,
      created_at: chat.created_at,
      message_count: chat
        .messages
        .iter()
        .filter(|m| !m.message.is_empty())
        .count(),
    });
  }
  summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
  Ok(summaries)
}

pub fn get_agent_chat(repo_path: &str, session_id: i64) -> Result<Option<AgentChat>, String> {
  let lock = chat_lock(repo_path, session_id);
  let _guard = lock.lock().map_err(|e| e.to_string())?;
  load_chat_unlocked(repo_path, session_id)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn sample_chat() -> AgentChat {
    AgentChat {
      session_id: 1,
      pty_session_id: "pty-1".into(),
      name: "Claude".into(),
      agent: AgentType::Claude,
      workspace_id: None,
      created_at: "2026-01-01T00:00:00Z".into(),
      screen_before_last_user_message: String::new(),
      messages: vec![ChatMessage {
        id: 0,
        role: ChatRole::Agent,
        message: String::new(),
        time: "t0".into(),
      }],
    }
  }

  #[test]
  fn initial_screen_is_first_agent_message() {
    let mut chat = sample_chat();
    let mut snap = None;
    note_screen(
      &mut chat,
      &mut snap,
      "Welcome to Claude\n───────────────\n>\n───────────────",
      "t1",
    );
    assert_eq!(chat.messages.len(), 1);
    assert_eq!(chat.messages[0].role, ChatRole::Agent);
    assert_eq!(chat.messages[0].message, "Welcome to Claude");
  }

  #[test]
  fn user_message_then_new_output_becomes_agent_reply() {
    let mut chat = sample_chat();
    let mut snap = None;
    note_screen(
      &mut chat,
      &mut snap,
      "Welcome\n───────────────\n>\n───────────────",
      "t1",
    );
    let before = "Welcome\n───────────────\n>\n───────────────";
    note_user_message(&mut chat, &mut snap, before, "fix the bug", "t2");
    note_screen(&mut chat, &mut snap,
      "Welcome\n───────────────\n> fix the bug\n───────────────\nI'll look at src/main.rs\n───────────────\n>\n───────────────",
      "t3",
    );
    assert_eq!(chat.messages.len(), 3);
    assert_eq!(chat.messages[1].role, ChatRole::User);
    assert_eq!(chat.messages[1].message, "fix the bug");
    assert_eq!(chat.messages[2].role, ChatRole::Agent);
    assert!(chat.messages[2]
      .message
      .contains("I'll look at src/main.rs"));
    assert!(!chat.messages[2].message.contains("fix the bug"));
  }

  #[test]
  fn later_screen_updates_create_separate_timestamped_messages() {
    let mut chat = sample_chat();
    let mut snap = None;
    let before = "hi";
    note_user_message(&mut chat, &mut snap, before, "hello", "t1");
    note_screen(&mut chat, &mut snap, "hi\npartial", "t2");
    note_screen(&mut chat, &mut snap, "hi\npartial reply done", "t3");
    let agents: Vec<_> = chat
      .messages
      .iter()
      .filter(|m| m.role == ChatRole::Agent)
      .collect();
    assert_eq!(agents[agents.len() - 2].message, "partial");
    assert_eq!(agents.last().unwrap().message, "reply done");
    assert_eq!(
      chat
        .messages
        .iter()
        .filter(|m| m.role == ChatRole::Agent)
        .count(),
      3
    );
  }

  #[test]
  fn strips_trailing_greater_than_input_box() {
    let formatted = format_agent_message(
      AgentType::Claude,
      "Answer here\n───────────────\n>\n───────────────",
      "",
    );
    assert_eq!(formatted, "Answer here");
  }

  #[test]
  fn empty_user_message_is_ignored() {
    let mut chat = sample_chat();
    let mut snap = None;
    note_user_message(&mut chat, &mut snap, "screen", "   ", "t1");
    assert_eq!(chat.messages.len(), 1);
  }

  #[test]
  fn register_and_list_skips_unregistered_shells() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 7, "pty-agent", "Claude", "claude", None, None).unwrap();
    let listed = list_agent_chats(&repo).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, 7);
    assert_eq!(listed[0].name, "Claude");
    assert!(get_agent_chat(&repo, 99).unwrap().is_none());
  }

  #[test]
  fn persist_round_trip_records_conversation() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 3, "pty-3", "Codex", "codex", Some(2), None).unwrap();
    record_screen(&repo, 3, "ready").unwrap();
    record_user_message(&repo, 3, "ready", "do the thing").unwrap();
    record_screen(&repo, 3, "ready\nworking on it").unwrap();
    let chat = get_agent_chat(&repo, 3).unwrap().unwrap();
    assert_eq!(chat.agent, AgentType::Codex);
    assert_eq!(chat.workspace_id, Some(2));
    assert!(chat
      .messages
      .iter()
      .any(|m| m.role == ChatRole::User && m.message == "do the thing"));
    assert!(chat
      .messages
      .iter()
      .any(|m| m.role == ChatRole::Agent && m.message.contains("working on it")));
  }

  #[test]
  fn repeated_screen_does_not_rewrite_the_file_and_writes_compact_json() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 4, "pty-4", "Claude", "claude", None, None).unwrap();
    record_screen(&repo, 4, "hello there").unwrap();
    let path = chat_path(&repo, 4);
    let first = fs::read_to_string(&path).unwrap();
    assert!(!first.contains('\n'), "chat log is compact JSON");

    // A sentinel only survives if the unchanged snapshot is not written back.
    fs::write(&path, format!("{first} ")).unwrap();
    record_screen(&repo, 4, "hello there").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{first} "));
  }

  #[test]
  fn cached_chat_is_reloaded_after_an_external_write() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 5, "pty-5", "Claude", "claude", None, None).unwrap();
    let before = get_agent_chat(&repo, 5).unwrap().unwrap();
    let mut edited = before;
    edited.name = "Renamed by another process".to_string();
    fs::write(chat_path(&repo, 5), serde_json::to_string(&edited).unwrap()).unwrap();
    assert_eq!(get_agent_chat(&repo, 5).unwrap().unwrap().name, edited.name);
  }

  #[test]
  fn record_without_register_returns_error() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let err = record_screen(&repo, 1, "x").unwrap_err();
    assert!(err.contains("not registered"));
  }

  #[test]
  fn registration_stores_non_empty_initial_prompt() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let chat = register_agent_chat(
      &repo,
      8,
      "pty-8",
      "Claude",
      "claude",
      None,
      Some("fix the live logs"),
    )
    .unwrap();

    assert_eq!(chat.messages.len(), 2);
    assert_eq!(chat.messages[0].role, ChatRole::Agent);
    assert_eq!(chat.messages[1].role, ChatRole::User);
    assert_eq!(chat.messages[1].message, "fix the live logs");
  }

  fn scrolling_screen(t: usize) -> String {
    let mut rows: Vec<String> = (0..20)
      .map(|row| format!("output line {}", t + row))
      .collect();
    rows.push(format!("✻ Thinking… ({t}s)"));
    rows.push("───────────────".to_string());
    rows.push(">".to_string());
    rows.push("───────────────".to_string());
    rows.join("\n")
  }

  #[test]
  fn scrolling_output_logs_only_the_lines_that_are_new() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 11, "pty-11", "Claude", "claude", None, None).unwrap();
    for t in 0..40 {
      record_screen(&repo, 11, &scrolling_screen(t)).unwrap();
    }
    let chat = get_agent_chat(&repo, 11).unwrap().unwrap();
    let logged: usize = chat.messages.iter().map(|m| m.message.len()).sum();
    // The first screen once, then a line and the spinner per tick.
    assert!(
      logged < 3_000,
      "logged {logged} bytes for 40 one-line scrolls"
    );
    let lines: std::collections::HashSet<&str> = chat
      .messages
      .iter()
      .flat_map(|m| m.message.lines())
      .collect();
    for line in 0..59 {
      assert!(
        lines.contains(format!("output line {line}").as_str()),
        "line {line} was lost"
      );
    }
  }

  #[test]
  fn appended_messages_leave_a_file_that_parses_as_the_chat() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 12, "pty-12", "Claude", "claude", None, None).unwrap();
    record_screen(&repo, 12, "first").unwrap();
    record_user_message(&repo, 12, "first", "do it").unwrap();
    record_screen(&repo, 12, "first\nsecond").unwrap();
    record_screen(&repo, 12, "first\nsecond\nthird").unwrap();
    let from_disk: AgentChat =
      serde_json::from_str(&fs::read_to_string(chat_path(&repo, 12)).unwrap()).unwrap();
    assert_eq!(from_disk, get_agent_chat(&repo, 12).unwrap().unwrap());
    assert_eq!(from_disk.messages.last().unwrap().message, "third");
    assert_eq!(from_disk.messages.len(), 4);
  }

  #[test]
  fn ending_a_session_drops_its_chat_from_memory_but_keeps_the_file() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 13, "pty-13", "Claude", "claude", None, None).unwrap();
    record_screen(&repo, 13, "hello").unwrap();
    let path = chat_path(&repo, 13);
    assert!(CHAT_CACHE
      .get()
      .unwrap()
      .lock()
      .unwrap()
      .contains_key(&path));
    end_session(&repo, 13);
    assert!(!CHAT_CACHE
      .get()
      .unwrap()
      .lock()
      .unwrap()
      .contains_key(&path));
    assert_eq!(
      get_agent_chat(&repo, 13).unwrap().unwrap().messages[0].message,
      "hello"
    );
  }

  #[test]
  fn repeated_registration_does_not_duplicate_initial_prompt() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    register_agent_chat(&repo, 9, "pty-9", "Codex", "codex", None, Some("only once")).unwrap();
    let chat =
      register_agent_chat(&repo, 9, "pty-9", "Codex", "codex", None, Some("only once")).unwrap();

    assert_eq!(
      chat
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::User && message.message == "only once")
        .count(),
      1
    );
  }
}
