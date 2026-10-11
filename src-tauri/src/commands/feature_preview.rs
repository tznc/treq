use crate::core::feature_preview::{self, PreviewFeature};
use crate::lock_ext::LockExt;
use crate::AppState;
use tauri::State;

pub fn require(state: &State<AppState>, feature: PreviewFeature) -> Result<(), String> {
  let db = state.db.lock_or_recover();
  feature_preview::require(&db, feature)
}

pub fn require_remote_for_endpoint(
  state: &State<AppState>,
  endpoint: &crate::core::remote_control_plane::SshEndpoint,
) -> Result<(), String> {
  let db = state.db.lock_or_recover();
  feature_preview::require_remote_for_source(&db, &endpoint.source)
}
