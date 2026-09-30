use std::collections::HashMap;
use std::io::{Read, Write};

use tauri::{AppHandle, Emitter, EventTarget, Manager};

use crate::agent_dispatch;
use crate::local_db;
use crate::AppState;

pub(crate) fn extract_repo_from_agent_deep_link(url: &str) -> Option<String> {
  let prefix = "treq://agent/start?";
  if !url.starts_with(prefix) {
    return None;
  }
  let query = &url[prefix.len()..];
  for pair in query.split('&') {
    let (key, value) = pair.split_once('=')?;
    if key != "repo" {
      continue;
    }
    let decoded = urlencoding::decode(value).ok()?;
    return Some(decoded.into_owned());
  }
  None
}

fn find_target_window_label_for_repo(app: &AppHandle, repo: &str) -> Option<String> {
  let state = app.state::<AppState>();
  let normalized_target = agent_dispatch::normalize_repo_path(repo);
  let windows = app.webview_windows();

  let pick = |labels: Vec<String>| -> Option<String> {
    let mut fallback: Option<String> = None;
    for label in labels {
      let Some(window) = windows.get(&label) else {
        continue;
      };
      if window.is_focused().unwrap_or(false) {
        return Some(label);
      }
      if fallback.is_none() {
        fallback = Some(label);
      }
    }
    fallback
  };

  let main_labels: Vec<String> = state
    .window_repo_paths
    .lock()
    .ok()?
    .iter()
    .filter(|(_, path)| agent_dispatch::normalize_repo_path(path) == normalized_target)
    .map(|(label, _)| label.clone())
    .collect();
  if let Some(label) = pick(main_labels) {
    return Some(label);
  }

  // A window where the repository is a supporting repository handles it only
  // when no window has it open as the main repository.
  let supporting_labels: Vec<String> = state
    .window_supporting_repo_paths
    .lock()
    .ok()?
    .iter()
    .filter(|(_, paths)| {
      paths
        .iter()
        .any(|path| agent_dispatch::normalize_repo_path(path) == normalized_target)
    })
    .map(|(label, _)| label.clone())
    .collect();
  pick(supporting_labels)
}

pub(crate) fn route_agent_deep_link(app: &AppHandle, url: String) -> bool {
  if let Some(repo) = extract_repo_from_agent_deep_link(&url) {
    if let Some(label) = find_target_window_label_for_repo(app, &repo) {
      let _ = app.emit_to(
        EventTarget::webview_window(&label),
        "deep-link-received",
        vec![url],
      );
      return true;
    }
  }
  false
}

pub(crate) fn parse_agent_request_from_url(
  url: &str,
) -> Option<agent_dispatch::AgentDispatchRequest> {
  let prefix = "treq://agent/start?";
  if !url.starts_with(prefix) {
    return None;
  }
  let mut values = std::collections::HashMap::new();
  for pair in url[prefix.len()..].split('&') {
    let (key, value) = pair.split_once('=')?;
    let decoded = urlencoding::decode(value).ok()?.into_owned();
    values.insert(key.to_string(), decoded);
  }
  Some(agent_dispatch::AgentDispatchRequest {
    request_id: values.get("request_id")?.clone(),
    repo: values.get("repo")?.clone(),
    branch: values.get("branch")?.clone(),
    prompt: values.get("prompt")?.clone(),
    mode: values.get("mode")?.clone(),
    agent: values.get("agent")?.clone(),
  })
}

pub(crate) fn build_agent_deep_link_url(request: &agent_dispatch::AgentDispatchRequest) -> String {
  format!(
    "treq://agent/start?repo={}&branch={}&prompt={}&mode={}&agent={}&request_id={}",
    urlencoding::encode(&request.repo),
    urlencoding::encode(&request.branch),
    urlencoding::encode(&request.prompt),
    urlencoding::encode(&request.mode),
    urlencoding::encode(&request.agent),
    urlencoding::encode(&request.request_id),
  )
}

pub(crate) fn route_agent_dispatch_request(
  app: &AppHandle,
  request: &agent_dispatch::AgentDispatchRequest,
) -> agent_dispatch::AgentDispatchResponse {
  let url = build_agent_deep_link_url(request);
  if let Some(label) = find_target_window_label_for_repo(app, &request.repo) {
    let _ = app.emit_to(
      EventTarget::webview_window(&label),
      "deep-link-received",
      vec![url],
    );
    return agent_dispatch::AgentDispatchResponse::handled();
  }
  agent_dispatch::AgentDispatchResponse::not_handled(format!(
    "no matching window for repo '{}' request_id '{}'",
    request.repo, request.request_id
  ))
}

pub(crate) fn route_send_dispatch_request(
  app: &AppHandle,
  request: &crate::send_dispatch::SendDispatchRequest,
) -> agent_dispatch::AgentDispatchResponse {
  if let Some(label) = find_target_window_label_for_repo(app, &request.repo) {
    let _ = app.emit_to(
      EventTarget::webview_window(&label),
      "treq-send-received",
      request.clone(),
    );
    return agent_dispatch::AgentDispatchResponse::handled();
  }
  agent_dispatch::AgentDispatchResponse::not_handled(format!(
    "no matching window for repo '{}' request_id '{}'",
    request.repo, request.request_id
  ))
}

/// Shows a `treq notify` request as an OS notification unless a Treq window
/// is focused or the user turned agent notifications off.
pub(crate) fn route_notify_dispatch_request(
  app: &AppHandle,
  request: &crate::notify_dispatch::NotifyDispatchRequest,
) -> agent_dispatch::AgentDispatchResponse {
  use crate::lock_ext::LockExt;
  use crate::notify_dispatch::{decide, NotifyDecision, NOTIFY_SETTING_KEY};
  use tauri_plugin_notification::{NotificationExt, PermissionState};

  let any_window_focused = app
    .webview_windows()
    .values()
    .any(|window| window.is_focused().unwrap_or(false));
  let setting = app
    .state::<AppState>()
    .db
    .lock_or_recover()
    .get_setting(NOTIFY_SETTING_KEY)
    .ok()
    .flatten();
  let notification = match decide(request, any_window_focused, setting.as_deref()) {
    NotifyDecision::Skip(reason) => return agent_dispatch::AgentDispatchResponse::skipped(reason),
    NotifyDecision::Show(notification) => notification,
  };

  let notifier = app.notification();
  // Desktop always reports Granted. Mobile prompts until the user decides.
  if !matches!(notifier.permission_state(), Ok(PermissionState::Granted)) {
    let _ = notifier.request_permission();
  }
  match notifier
    .builder()
    .title(notification.title)
    .body(notification.body)
    .show()
  {
    Ok(()) => agent_dispatch::AgentDispatchResponse::handled(),
    Err(error) => {
      agent_dispatch::AgentDispatchResponse::error(format!("failed to show notification: {error}"))
    }
  }
}

pub(crate) fn start_agent_ipc_listener(app: AppHandle, listener: std::net::TcpListener) {
  std::thread::spawn(move || {
    for stream in listener.incoming() {
      let Ok(mut stream) = stream else {
        continue;
      };

      let mut payload = String::new();
      if stream.read_to_string(&mut payload).is_err() {
        let _ = stream.write_all(
          serde_json::to_string(&agent_dispatch::AgentDispatchResponse::error(
            "invalid request payload",
          ))
          .unwrap_or_else(|_| {
            "{\"status\":\"error\",\"reason\":\"invalid request payload\"}".to_string()
          })
          .as_bytes(),
        );
        continue;
      }

      let response = match crate::send_dispatch::parse_ipc_payload(payload.trim()) {
        Ok(crate::send_dispatch::IpcDispatchMessage::Agent(request)) => {
          route_agent_dispatch_request(&app, &request)
        }
        Ok(crate::send_dispatch::IpcDispatchMessage::Send(request)) => {
          route_send_dispatch_request(&app, &request)
        }
        Ok(crate::send_dispatch::IpcDispatchMessage::Notify(request)) => {
          route_notify_dispatch_request(&app, &request)
        }
        Err(error) => agent_dispatch::AgentDispatchResponse::error(error),
      };
      let response_json = serde_json::to_string(&response).unwrap_or_else(|_| {
        "{\"status\":\"error\",\"reason\":\"failed to serialize response\"}".to_string()
      });
      let _ = stream.write_all(response_json.as_bytes());
    }
  });
}

pub(crate) fn start_instance_registry_heartbeat(app: AppHandle) {
  std::thread::spawn(move || loop {
    let state = app.state::<AppState>();
    let now = agent_dispatch::now_millis();

    let repo_map = state.window_repo_paths.lock().ok().map(|m| m.clone());
    let supporting_map = state
      .window_supporting_repo_paths
      .lock()
      .ok()
      .map(|m| m.clone())
      .unwrap_or_default();
    let mut focus_map = match state.window_last_focused_at.lock() {
      Ok(guard) => guard,
      Err(_) => {
        std::thread::sleep(std::time::Duration::from_millis(
          agent_dispatch::HEARTBEAT_INTERVAL_MS,
        ));
        continue;
      }
    };
    let windows = app.webview_windows();
    let mut snapshots = Vec::new();
    if let Some(repo_map) = repo_map {
      for (label, repo_path) in repo_map {
        let focused = windows
          .get(&label)
          .and_then(|w| w.is_focused().ok())
          .unwrap_or(false);
        if focused {
          focus_map.insert(label.clone(), now);
        }
        let last_focused_at = focus_map.get(&label).copied();
        for supporting_path in supporting_map.get(&label).into_iter().flatten() {
          snapshots.push(agent_dispatch::InstanceWindowSnapshot {
            window_label: label.clone(),
            normalized_repo_path: agent_dispatch::normalize_repo_path(supporting_path),
            focused,
            last_focused_at,
            supporting: true,
          });
        }
        snapshots.push(agent_dispatch::InstanceWindowSnapshot {
          window_label: label,
          normalized_repo_path: agent_dispatch::normalize_repo_path(&repo_path),
          focused,
          last_focused_at,
          supporting: false,
        });
      }
    }
    drop(focus_map);
    let mut windows_by_repo: HashMap<String, Vec<agent_dispatch::InstanceWindowSnapshot>> =
      HashMap::new();
    for snapshot in snapshots {
      windows_by_repo
        .entry(snapshot.normalized_repo_path.clone())
        .or_default()
        .push(snapshot);
    }

    for (repo_path, windows) in windows_by_repo {
      if repo_path.is_empty() {
        log::warn!("Skipping instance heartbeat for empty repo path");
        continue;
      }
      if let Err(error) = local_db::init_local_db(&repo_path) {
        log::warn!(
          "Skipping instance heartbeat for repo {}: {}",
          repo_path,
          error
        );
        continue;
      }
      if let Err(error) = local_db::prune_stale_instance_registry(
        &repo_path,
        now,
        agent_dispatch::HEARTBEAT_TIMEOUT_MS,
      ) {
        log::warn!(
          "Failed pruning stale instance registry rows for repo {}: {}",
          repo_path,
          error
        );
        continue;
      }
      if let Err(error) = local_db::upsert_instance_registry(
        &repo_path,
        agent_dispatch::RegisteredInstance {
          instance_id: state.dispatch_instance_id.clone(),
          pid: std::process::id(),
          started_at: state.dispatch_started_at,
          last_heartbeat_at: now,
          endpoint: state.dispatch_endpoint.clone(),
          windows,
        },
      ) {
        log::warn!(
          "Failed upserting instance registry row for repo {}: {}",
          repo_path,
          error
        );
      }
    }

    std::thread::sleep(std::time::Duration::from_millis(
      agent_dispatch::HEARTBEAT_INTERVAL_MS,
    ));
  });
}
