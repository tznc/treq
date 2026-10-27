//! Fakes of the coordinator's db and network, for tests.

use super::coordinator::{Ledger, Network};
use super::hub::{Envelope, Report, SessionInfo};
use super::model::State;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Default)]
pub struct FakeLedger {
  pub inserted: RefCell<Vec<(String, Envelope, Vec<(String, String)>)>>,
  pub updates: RefCell<Vec<(String, String, String, State, Option<String>)>>,
}

impl Ledger for FakeLedger {
  fn insert(&self, repo: &str, message: &Envelope, targets: &[(String, String)]) -> Result<(), String> {
    self
      .inserted
      .borrow_mut()
      .push((repo.into(), message.clone(), targets.to_vec()));
    Ok(())
  }
  fn update(
    &self,
    repo: &str,
    id: &str,
    target: &str,
    state: State,
    mode: Option<&str>,
    _now_ms: u64,
  ) -> Result<(), String> {
    self.updates.borrow_mut().push((
      repo.into(),
      id.into(),
      target.into(),
      state,
      mode.map(str::to_string),
    ));
    Ok(())
  }
}

#[derive(Default)]
pub struct FakeNetwork {
  pub remote: HashMap<String, Vec<SessionInfo>>,
  pub down: Vec<String>,
  pub delivered: RefCell<Vec<(String, Envelope)>>,
  pub reported: RefCell<Vec<(String, Report)>>,
  pub mirrored: RefCell<Vec<Envelope>>,
  pub mirror_down: bool,
}

impl Network for FakeNetwork {
  fn own_endpoint(&self) -> String {
    "127.0.0.1:1".into()
  }
  fn peers(&self, _repos: &[String]) -> Vec<String> {
    let mut endpoints: Vec<String> = self.remote.keys().cloned().collect();
    endpoints.sort();
    endpoints
  }
  fn list(&self, endpoint: &str) -> Result<Vec<SessionInfo>, String> {
    self
      .remote
      .get(endpoint)
      .cloned()
      .ok_or_else(|| "unreachable".to_string())
  }
  fn deliver(&self, endpoint: &str, envelope: &Envelope) -> Result<(), String> {
    if self.down.iter().any(|down| down == endpoint) {
      return Err("connection refused".into());
    }
    self
      .delivered
      .borrow_mut()
      .push((endpoint.into(), envelope.clone()));
    Ok(())
  }
  fn mirror(&self, envelope: &Envelope) -> Result<(), String> {
    if self.mirror_down {
      return Err("host unreachable".into());
    }
    self.mirrored.borrow_mut().push(envelope.clone());
    Ok(())
  }

  fn report(&self, endpoint: &str, report: &Report) -> Result<(), String> {
    self
      .reported
      .borrow_mut()
      .push((endpoint.into(), report.clone()));
    Ok(())
  }
}

