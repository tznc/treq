use crate::tracker::{KickoffLedger, KickoffPoller, MAX_KICKOFF_ATTEMPTS};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearIssue {
  pub id: String,
  pub identifier: String,
  pub title: String,
  pub description: Option<String>,
  pub state: LinearState,
  pub labels: Vec<String>,
  pub branch_name: String,
  pub parent_id: Option<String>,
  pub sub_issue_ids: Vec<String>,
  pub url: String,
  pub assignee: Option<LinearUser>,
  pub priority: i32,
  pub priority_label: String,
  pub project: Option<LinearProjectRef>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearTeam {
  pub id: String,
  pub name: String,
  pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearState {
  pub name: String,
  #[serde(rename = "type")]
  pub state_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearUser {
  pub id: String,
  pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearProjectRef {
  pub id: String,
  pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearProject {
  pub id: String,
  pub name: String,
  pub description: Option<String>,
  pub state: String,
  pub target_date: Option<String>,
  pub progress: f64,
  pub url: String,
  pub lead: Option<LinearUser>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearDocument {
  pub id: String,
  pub title: String,
  pub content: Option<String>,
  pub url: String,
  pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinearComment {
  pub id: String,
  pub body: String,
  pub user: Option<LinearUser>,
  pub created_at: String,
  /// The excerpt of the document/project body this comment is anchored to,
  /// when Linear reports one (inline "highlight and comment" threads).
  pub quoted_text: Option<String>,
}

pub enum LinearClientSource {
  /// A personal API key from the repo's Linear settings, sent straight to
  /// Linear.
  ApiKey(String),
  /// The signed-in treq user's Linear OAuth grant, used through the
  /// `linear-proxy` Edge Function, which holds the Linear token.
  Proxy(LinearProxySession),
}

#[derive(Clone)]
pub struct LinearProxySession {
  pub supabase_url: String,
  pub access_token: String,
}

// The frontend pushes the Supabase session on sign-in and every token
// refresh. The auto-kickoff poller reads it here too, so OAuth-connected
// repos work without a Tauri command in flight. It is never logged.
static PROXY_SESSION: RwLock<Option<LinearProxySession>> = RwLock::new(None);

/// Sets, or clears on sign-out, the session used for the OAuth proxy.
pub fn set_proxy_session(supabase_url: Option<String>, access_token: Option<String>) {
  let session = match (supabase_url, access_token) {
    (Some(url), Some(token)) if !url.trim().is_empty() && !token.is_empty() => {
      Some(LinearProxySession {
        supabase_url: url.trim().trim_end_matches('/').to_string(),
        access_token: token,
      })
    }
    _ => None,
  };
  match PROXY_SESSION.write() {
    Ok(mut guard) => *guard = session,
    Err(poisoned) => *poisoned.into_inner() = session,
  }
}

fn proxy_session() -> Option<LinearProxySession> {
  match PROXY_SESSION.read() {
    Ok(guard) => guard.clone(),
    Err(poisoned) => poisoned.into_inner().clone(),
  }
}

pub fn resolve_linear_client(
  repo_path: &str,
  db: &crate::db::Database,
) -> Result<LinearClientSource, String> {
  let api_key = crate::tracker::read_setting(db, repo_path, "linear_api_key")?;
  resolve_client_source(api_key, proxy_session())
}

/// A repo API key wins over OAuth so a repo can target a different Linear
/// workspace than the one the user connected.
fn resolve_client_source(
  api_key: Option<String>,
  session: Option<LinearProxySession>,
) -> Result<LinearClientSource, String> {
  match (api_key, session) {
    (Some(key), _) => Ok(LinearClientSource::ApiKey(key)),
    (None, Some(session)) => Ok(LinearClientSource::Proxy(session)),
    (None, None) => Err(
      "Linear is not connected. Add an API key in the Linear settings, or sign in to treq and connect Linear with OAuth."
        .to_string(),
    ),
  }
}

#[derive(Deserialize)]
struct LinearGraphqlResponse<T> {
  data: Option<T>,
  errors: Option<Vec<LinearGraphqlError>>,
}

#[derive(Deserialize)]
struct LinearGraphqlError {
  message: String,
}

const LINEAR_GRAPHQL_URL: &str = "https://api.linear.app/graphql";

// reqwest has no default timeout. Without one, a stalled Linear request holds
// the auto-kickoff poller thread forever and stops kickoffs for every repo.
// Connecting fails fast; a connected request may take a while, since a large
// issue page or a busy proxy can be slow to answer.
const LINEAR_REQUEST_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LINEAR_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// Tests point requests at a local mock server and shorten the timeout.
// `#[tokio::test]` runs on one thread, so a thread-local stays test-scoped.
#[cfg(test)]
thread_local! {
  static TEST_ENDPOINT: std::cell::RefCell<Option<(String, Duration)>> =
    const { std::cell::RefCell::new(None) };
}

fn linear_endpoint() -> (String, Duration) {
  #[cfg(test)]
  if let Some(endpoint) = TEST_ENDPOINT.with(|e| e.borrow().clone()) {
    return endpoint;
  }
  (LINEAR_GRAPHQL_URL.to_string(), LINEAR_REQUEST_TIMEOUT)
}

fn linear_http_client() -> &'static reqwest::Client {
  static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
  CLIENT.get_or_init(|| {
    reqwest::Client::builder()
      .connect_timeout(LINEAR_CONNECT_TIMEOUT)
      .build()
      .unwrap_or_else(|_| reqwest::Client::new())
  })
}

/// Sends one GraphQL request to Linear and returns its `data`. `what` names
/// the resource in error messages ("issues", "teams").
async fn linear_graphql<T: DeserializeOwned>(
  client: &LinearClientSource,
  body: &serde_json::Value,
  what: &str,
) -> Result<T, String> {
  let (url, timeout) = linear_endpoint();
  let request = match client {
    LinearClientSource::ApiKey(api_key) => linear_http_client()
      .post(url)
      .header("Authorization", api_key),
    LinearClientSource::Proxy(session) => linear_http_client()
      .post(format!(
        "{}/functions/v1/linear-proxy",
        session.supabase_url
      ))
      .bearer_auth(&session.access_token),
  };
  let response = request
    .timeout(timeout)
    .json(body)
    .send()
    .await
    .map_err(|e| {
      if e.is_timeout() {
        format!("Linear request for {what} timed out")
      } else {
        format!("Failed to fetch Linear {what}: {e}")
      }
    })?;

  let status = response.status();
  let text = response
    .text()
    .await
    .map_err(|e| format!("Failed to read Linear response: {e}"))?;
  parse_graphql_response(
    status,
    &text,
    matches!(client, LinearClientSource::Proxy(_)),
  )
}

/// Linear reports query errors as a GraphQL `errors` array, often with a 4xx
/// status. Those messages are the most useful, so they win when present. A
/// body that is not GraphQL JSON (a proxy error page, a rate-limit response)
/// falls back to a message built from the HTTP status.
fn parse_graphql_response<T: DeserializeOwned>(
  status: reqwest::StatusCode,
  text: &str,
  via_proxy: bool,
) -> Result<T, String> {
  let result = match serde_json::from_str::<LinearGraphqlResponse<T>>(text) {
    Ok(result) => result,
    Err(e) if status.is_success() => return Err(format!("Failed to parse Linear response: {e}")),
    Err(_) => return Err(http_status_error(status, text, via_proxy)),
  };

  if let Some(errors) = result.errors.filter(|errors| !errors.is_empty()) {
    return Err(format!(
      "Linear API error: {}",
      errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
    ));
  }
  if !status.is_success() {
    return Err(http_status_error(status, text, via_proxy));
  }

  result
    .data
    .ok_or_else(|| "No data in Linear response".to_string())
}

fn http_status_error(status: reqwest::StatusCode, text: &str, via_proxy: bool) -> String {
  // The proxy explains its own failures ("Linear account not linked") in an
  // `error` field. Those beat a generic status message.
  if via_proxy {
    if let Some(message) = serde_json::from_str::<serde_json::Value>(text)
      .ok()
      .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
    {
      return format!("Linear: {message}");
    }
  }
  match status.as_u16() {
    401 | 403 if via_proxy => {
      format!(
        "treq could not authenticate with the Linear proxy ({status}). Sign in to treq again."
      )
    }
    401 | 403 => {
      format!(
        "Linear rejected the API key ({status}). Check the Linear settings for this repository."
      )
    }
    429 => format!("Linear rate limit reached ({status}). Try again in a minute."),
    _ => {
      let body: String = text.chars().take(300).collect();
      format!("Linear API error ({status}): {body}")
    }
  }
}

#[derive(Deserialize)]
struct LinearIssuesData {
  issues: LinearIssuesConnection,
}

#[derive(Deserialize)]
struct LinearIssuesConnection {
  nodes: Vec<LinearIssueNode>,
  #[serde(default, rename = "pageInfo")]
  page_info: LinearPageInfo,
}

#[derive(Deserialize, Default)]
struct LinearPageInfo {
  #[serde(default, rename = "hasNextPage")]
  has_next_page: bool,
  #[serde(default, rename = "endCursor")]
  end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct LinearIssueNode {
  id: String,
  identifier: String,
  title: String,
  description: Option<String>,
  #[serde(default)]
  state: LinearStateData,
  #[serde(default)]
  labels: LinearLabelsConnection,
  #[serde(rename = "branchName")]
  branch_name: String,
  #[serde(rename = "parent")]
  parent_id: Option<LinearParent>,
  #[serde(default, rename = "children")]
  sub_issues: LinearSubIssuesConnection,
  url: String,
  #[serde(default)]
  assignee: Option<LinearUserNode>,
  #[serde(default)]
  priority: i32,
  #[serde(default, rename = "priorityLabel")]
  priority_label: String,
  #[serde(default)]
  project: Option<LinearProjectRefNode>,
}

#[derive(Deserialize)]
struct LinearUserNode {
  id: String,
  name: String,
}

#[derive(Deserialize)]
struct LinearProjectRefNode {
  id: String,
  name: String,
}

#[derive(Deserialize)]
struct LinearStateData {
  name: String,
  #[serde(rename = "type")]
  state_type: String,
}

impl Default for LinearStateData {
  fn default() -> Self {
    Self {
      name: "Unknown".to_string(),
      state_type: "unknown".to_string(),
    }
  }
}

#[derive(Deserialize, Default)]
struct LinearLabelsConnection {
  nodes: Vec<LinearLabelNode>,
}

#[derive(Deserialize)]
struct LinearLabelNode {
  name: String,
}

#[derive(Deserialize)]
struct LinearParent {
  id: String,
}

#[derive(Deserialize, Default)]
struct LinearSubIssuesConnection {
  nodes: Vec<LinearSubIssueNode>,
}

#[derive(Deserialize)]
struct LinearSubIssueNode {
  id: String,
}

#[derive(Deserialize)]
struct LinearTeamsData {
  teams: LinearTeamsConnection,
}

#[derive(Deserialize)]
struct LinearTeamsConnection {
  nodes: Vec<LinearTeamNode>,
}

#[derive(Deserialize)]
struct LinearTeamNode {
  id: String,
  name: String,
  key: String,
}

pub async fn linear_list_teams_impl(
  client: &LinearClientSource,
) -> Result<Vec<LinearTeam>, String> {
  let query = r#"query {
    teams(first: 100) {
      nodes {
        id
        name
        key
      }
    }
  }"#;

  let data: LinearTeamsData =
    linear_graphql(client, &serde_json::json!({ "query": query }), "teams").await?;

  Ok(
    data
      .teams
      .nodes
      .into_iter()
      .map(|node| LinearTeam {
        id: node.id,
        name: node.name,
        key: node.key,
      })
      .collect(),
  )
}

// Caller-supplied values (team keys, entity IDs) always travel as GraphQL
// variables. Splicing them into the query text lets a quote end the string
// literal and inject query syntax.
const ISSUE_FIELDS: &str = r#"
  id
  identifier
  title
  description
  state { name type }
  labels(first: 50) { nodes { name } }
  branchName
  parent { id }
  children(first: 50) { nodes { id } }
  url
  assignee { id name }
  priority
  priorityLabel
  project { id name }
"#;

const ISSUE_PAGE_SIZE: u32 = 100;
// 10 pages keeps a huge workspace from stalling the panel. Issues come back
// most recently updated first, so the cap drops the stalest ones.
const MAX_ISSUE_PAGES: usize = 10;

/// Linear `IssueFilter` for the issue list. `None` means no filter.
fn team_issue_filter(team_filter: Option<&str>) -> Option<serde_json::Value> {
  team_filter.map(|team| serde_json::json!({ "team": { "key": { "eq": team } } }))
}

fn list_issues_body(filter: Option<&serde_json::Value>, after: Option<&str>) -> serde_json::Value {
  issue_page_body(ISSUE_FIELDS, filter, after)
}

fn issue_page_body(
  fields: &str,
  filter: Option<&serde_json::Value>,
  after: Option<&str>,
) -> serde_json::Value {
  serde_json::json!({
    "query": format!(
      "query($first: Int!, $after: String, $filter: IssueFilter) {{ issues(first: $first, after: $after, filter: $filter, orderBy: updatedAt) {{ nodes {{ {fields} }} pageInfo {{ hasNextPage endCursor }} }} }}"
    ),
    "variables": {
      "first": ISSUE_PAGE_SIZE,
      "after": after,
      "filter": filter,
    },
  })
}

/// Follows `pageInfo` cursors until Linear reports no next page or the
/// `MAX_ISSUE_PAGES` cap is hit.
async fn fetch_issue_pages(
  client: &LinearClientSource,
  filter: Option<&serde_json::Value>,
) -> Result<Vec<LinearIssue>, String> {
  let mut issues = Vec::new();
  let mut after: Option<String> = None;
  for _ in 0..MAX_ISSUE_PAGES {
    let data: LinearIssuesData = linear_graphql(
      client,
      &list_issues_body(filter, after.as_deref()),
      "issues",
    )
    .await?;
    let page = data.issues;
    issues.extend(page.nodes.into_iter().map(map_issue_node));
    match next_cursor(page.page_info) {
      Some(cursor) => after = Some(cursor),
      None => return Ok(issues),
    }
  }
  log::warn!(
    "linear: issue list truncated at {} issues",
    MAX_ISSUE_PAGES * ISSUE_PAGE_SIZE as usize
  );
  Ok(issues)
}

// The poller prunes its ledger against this list, so a partial list would
// make it forget handled issues and kick them off again. The cap is high and
// exceeding it is an error rather than a silent truncation.
const MAX_LABELED_ISSUE_PAGES: usize = 50;

/// Open issues carrying `label`, for auto-kickoff. Filtering happens in
/// Linear so issues outside the panel's page window are still found, and
/// completed or canceled issues never start a workspace.
fn kickoff_issue_filter(label: &str) -> serde_json::Value {
  serde_json::json!({
    "labels": { "some": { "name": { "eqIgnoreCase": label } } },
    "state": { "type": { "nin": ["completed", "canceled"] } },
  })
}

#[derive(Deserialize)]
struct LinearIssueIdsData {
  issues: LinearIssueIdsConnection,
}

#[derive(Deserialize)]
struct LinearIssueIdsConnection {
  nodes: Vec<LinearSubIssueNode>,
  #[serde(default, rename = "pageInfo")]
  page_info: LinearPageInfo,
}

pub async fn linear_list_labeled_issue_ids_impl(
  client: &LinearClientSource,
  label: &str,
) -> Result<Vec<String>, String> {
  let filter = kickoff_issue_filter(label);
  let mut ids = Vec::new();
  let mut after: Option<String> = None;
  for _ in 0..MAX_LABELED_ISSUE_PAGES {
    let data: LinearIssueIdsData = linear_graphql(
      client,
      &issue_page_body("id", Some(&filter), after.as_deref()),
      "labeled issues",
    )
    .await?;
    let page = data.issues;
    ids.extend(page.nodes.into_iter().map(|node| node.id));
    match next_cursor(page.page_info) {
      Some(cursor) => after = Some(cursor),
      None => return Ok(ids),
    }
  }
  Err(format!(
    "More than {} open issues carry the label \"{label}\"; skipping auto-kickoff",
    MAX_LABELED_ISSUE_PAGES * ISSUE_PAGE_SIZE as usize
  ))
}

fn next_cursor(page_info: LinearPageInfo) -> Option<String> {
  page_info
    .has_next_page
    .then_some(page_info.end_cursor)
    .flatten()
}

fn get_issue_body(issue_id: &str) -> serde_json::Value {
  serde_json::json!({
    "query": format!("query($id: String!) {{ issue(id: $id) {{ {ISSUE_FIELDS} }} }}"),
    "variables": { "id": issue_id },
  })
}

fn project_documents_body(project_id: &str) -> serde_json::Value {
  serde_json::json!({
    "query": r#"query($id: String!) {
      project(id: $id) {
        documents(first: 100) {
          nodes {
            id
            title
            content
            url
            updatedAt
          }
        }
      }
    }"#,
    "variables": { "id": project_id },
  })
}

#[derive(Clone, Copy)]
enum CommentEntity {
  Issue,
  Project,
  Document,
}

impl CommentEntity {
  fn field(self) -> &'static str {
    match self {
      CommentEntity::Issue => "issue",
      CommentEntity::Project => "project",
      CommentEntity::Document => "document",
    }
  }
}

fn entity_comments_body(entity: CommentEntity, entity_id: &str) -> serde_json::Value {
  let field = entity.field();
  serde_json::json!({
    "query": format!(
      "query($id: String!) {{ {field}(id: $id) {{ comments(first: 100) {{ nodes {{ id body user {{ id name }} createdAt quotedText }} }} }} }}"
    ),
    "variables": { "id": entity_id },
  })
}

pub async fn linear_list_issues_impl(
  client: &LinearClientSource,
  team_filter: Option<&str>,
) -> Result<Vec<LinearIssue>, String> {
  fetch_issue_pages(client, team_issue_filter(team_filter).as_ref()).await
}

fn map_issue_node(node: LinearIssueNode) -> LinearIssue {
  LinearIssue {
    id: node.id,
    identifier: node.identifier,
    title: node.title,
    description: node.description,
    state: LinearState {
      name: node.state.name,
      state_type: node.state.state_type,
    },
    labels: node.labels.nodes.into_iter().map(|l| l.name).collect(),
    branch_name: node.branch_name,
    parent_id: node.parent_id.map(|p| p.id),
    sub_issue_ids: node.sub_issues.nodes.into_iter().map(|s| s.id).collect(),
    url: node.url,
    assignee: node.assignee.map(|a| LinearUser {
      id: a.id,
      name: a.name,
    }),
    priority: node.priority,
    priority_label: node.priority_label,
    project: node.project.map(|p| LinearProjectRef {
      id: p.id,
      name: p.name,
    }),
  }
}

pub async fn linear_get_issue_impl(
  client: &LinearClientSource,
  issue_id: &str,
) -> Result<LinearIssue, String> {
  #[derive(Deserialize)]
  struct IssueData {
    issue: Option<LinearIssueNode>,
  }

  let data: IssueData = linear_graphql(client, &get_issue_body(issue_id), "issue").await?;
  let node = data
    .issue
    .ok_or_else(|| format!("Issue {issue_id} not found"))?;

  Ok(map_issue_node(node))
}

pub async fn linear_get_viewer_impl(client: &LinearClientSource) -> Result<LinearUser, String> {
  let query = r#"query { viewer { id name } }"#;

  #[derive(Deserialize)]
  struct ViewerData {
    viewer: LinearUserNode,
  }

  let data: ViewerData =
    linear_graphql(client, &serde_json::json!({ "query": query }), "viewer").await?;

  Ok(LinearUser {
    id: data.viewer.id,
    name: data.viewer.name,
  })
}

pub async fn linear_list_projects_impl(
  client: &LinearClientSource,
) -> Result<Vec<LinearProject>, String> {
  let query = r#"query {
    projects(first: 100) {
      nodes {
        id
        name
        description
        state
        targetDate
        progress
        url
        lead { id name }
      }
    }
  }"#;

  #[derive(Deserialize)]
  struct ProjectsData {
    projects: ProjectsConnection,
  }

  #[derive(Deserialize)]
  struct ProjectsConnection {
    nodes: Vec<ProjectNode>,
  }

  #[derive(Deserialize)]
  struct ProjectNode {
    id: String,
    name: String,
    description: Option<String>,
    state: String,
    #[serde(rename = "targetDate")]
    target_date: Option<String>,
    #[serde(default)]
    progress: f64,
    url: String,
    #[serde(default)]
    lead: Option<LinearUserNode>,
  }

  let data: ProjectsData =
    linear_graphql(client, &serde_json::json!({ "query": query }), "projects").await?;

  Ok(
    data
      .projects
      .nodes
      .into_iter()
      .map(|node| LinearProject {
        id: node.id,
        name: node.name,
        description: node.description,
        state: node.state,
        target_date: node.target_date,
        progress: node.progress,
        url: node.url,
        lead: node.lead.map(|l| LinearUser {
          id: l.id,
          name: l.name,
        }),
      })
      .collect(),
  )
}

pub async fn linear_list_project_documents_impl(
  client: &LinearClientSource,
  project_id: &str,
) -> Result<Vec<LinearDocument>, String> {
  #[derive(Deserialize)]
  struct ProjectDocumentsData {
    project: Option<ProjectDocumentsNode>,
  }

  #[derive(Deserialize)]
  struct ProjectDocumentsNode {
    documents: DocumentsConnection,
  }

  #[derive(Deserialize)]
  struct DocumentsConnection {
    nodes: Vec<DocumentNode>,
  }

  #[derive(Deserialize)]
  struct DocumentNode {
    id: String,
    title: String,
    content: Option<String>,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
  }

  let data: ProjectDocumentsData =
    linear_graphql(client, &project_documents_body(project_id), "documents").await?;
  let project = data
    .project
    .ok_or_else(|| format!("Project {project_id} not found"))?;

  Ok(
    project
      .documents
      .nodes
      .into_iter()
      .map(|node| LinearDocument {
        id: node.id,
        title: node.title,
        content: node.content,
        url: node.url,
        updated_at: node.updated_at,
      })
      .collect(),
  )
}

#[derive(Deserialize)]
struct CommentNode {
  id: String,
  body: String,
  #[serde(default)]
  user: Option<LinearUserNode>,
  #[serde(rename = "createdAt")]
  created_at: String,
  #[serde(default, rename = "quotedText")]
  quoted_text: Option<String>,
}

#[derive(Deserialize)]
struct CommentsConnection {
  nodes: Vec<CommentNode>,
}

fn map_comment_node(node: CommentNode) -> LinearComment {
  LinearComment {
    id: node.id,
    body: node.body,
    user: node.user.map(|u| LinearUser {
      id: u.id,
      name: u.name,
    }),
    created_at: node.created_at,
    quoted_text: node.quoted_text,
  }
}

async fn fetch_comments_for_entity(
  client: &LinearClientSource,
  entity: CommentEntity,
  entity_id: &str,
) -> Result<Vec<LinearComment>, String> {
  let entity_field = entity.field();
  #[derive(Deserialize)]
  struct EntityCommentsData {
    #[serde(flatten)]
    entity: HashMap<String, Option<EntityCommentsNode>>,
  }

  #[derive(Deserialize)]
  struct EntityCommentsNode {
    comments: CommentsConnection,
  }

  let mut data: EntityCommentsData =
    linear_graphql(client, &entity_comments_body(entity, entity_id), "comments").await?;
  let node = data
    .entity
    .remove(entity_field)
    .flatten()
    .ok_or_else(|| format!("{entity_field} {entity_id} not found"))?;

  Ok(
    node
      .comments
      .nodes
      .into_iter()
      .map(map_comment_node)
      .collect(),
  )
}

pub async fn linear_list_issue_comments_impl(
  client: &LinearClientSource,
  issue_id: &str,
) -> Result<Vec<LinearComment>, String> {
  fetch_comments_for_entity(client, CommentEntity::Issue, issue_id).await
}

pub async fn linear_list_project_comments_impl(
  client: &LinearClientSource,
  project_id: &str,
) -> Result<Vec<LinearComment>, String> {
  fetch_comments_for_entity(client, CommentEntity::Project, project_id).await
}

pub async fn linear_list_document_comments_impl(
  client: &LinearClientSource,
  document_id: &str,
) -> Result<Vec<LinearComment>, String> {
  fetch_comments_for_entity(client, CommentEntity::Document, document_id).await
}

/// Where one reviewable Linear text lives: the query root that reads it, the
/// field holding it, the mutation that writes it, and the entity id.
struct ReviewTextLocation {
  root: &'static str,
  field: &'static str,
  mutation: &'static str,
  id: String,
}

fn review_text_location(
  target_type: &str,
  target_id: &str,
  text: &crate::core::linear_review::LinearTextRef,
) -> Result<ReviewTextLocation, String> {
  use crate::core::linear_review::{
    LinearTextRef, TARGET_LINEAR_DOCUMENT, TARGET_LINEAR_ISSUE, TARGET_LINEAR_PROJECT,
  };
  let (root, field, mutation, id) = match (target_type, text) {
    (_, LinearTextRef::Comment(id)) => ("comment", "body", "commentUpdate", id.as_str()),
    (TARGET_LINEAR_ISSUE, _) => ("issue", "description", "issueUpdate", target_id),
    (TARGET_LINEAR_PROJECT, _) => ("project", "description", "projectUpdate", target_id),
    (TARGET_LINEAR_DOCUMENT, _) => ("document", "content", "documentUpdate", target_id),
    (other, _) => return Err(format!("'{other}' is not a Linear review target")),
  };
  Ok(ReviewTextLocation {
    root,
    field,
    mutation,
    id: id.to_string(),
  })
}

fn get_review_text_body(location: &ReviewTextLocation) -> serde_json::Value {
  let ReviewTextLocation { root, field, .. } = location;
  serde_json::json!({
    "query": format!("query($id: String!) {{ {root}(id: $id) {{ {field} }} }}"),
    "variables": { "id": location.id },
  })
}

fn set_review_text_body(location: &ReviewTextLocation, text: &str) -> serde_json::Value {
  let ReviewTextLocation {
    field, mutation, ..
  } = location;
  serde_json::json!({
    "query": format!(
      "mutation($id: String!, $text: String!) {{ {mutation}(id: $id, input: {{ {field}: $text }}) {{ success }} }}"
    ),
    "variables": { "id": location.id, "text": text },
  })
}

/// Reads the current Linear text a review comment was written against.
async fn linear_get_review_text(
  client: &LinearClientSource,
  location: &ReviewTextLocation,
) -> Result<String, String> {
  let data: serde_json::Value =
    linear_graphql(client, &get_review_text_body(location), location.root).await?;
  let node = data
    .get(location.root)
    .filter(|node| !node.is_null())
    .ok_or_else(|| format!("{} {} not found", location.root, location.id))?;
  Ok(
    node[location.field]
      .as_str()
      .unwrap_or_default()
      .to_string(),
  )
}

async fn linear_set_review_text(
  client: &LinearClientSource,
  location: &ReviewTextLocation,
  text: &str,
) -> Result<(), String> {
  let data: serde_json::Value =
    linear_graphql(client, &set_review_text_body(location, text), location.root).await?;
  if data[location.mutation]["success"].as_bool() == Some(true) {
    Ok(())
  } else {
    Err(format!(
      "Linear did not accept the {} update",
      location.root
    ))
  }
}

/// Applies one agent review suggestion to Linear: re-reads the live text,
/// replaces the passage the comment reviewed, writes it back, and marks the
/// comment resolved. Editing a Linear comment only works on the viewer's own
/// comments; Linear's error is passed through for anyone else's.
pub async fn linear_apply_review_suggestion_impl(
  client: &LinearClientSource,
  repo_path: &str,
  comment_id: &str,
) -> Result<(), String> {
  use crate::core::linear_review::{is_linear_target, parse_text_ref, splice_quoted};
  let comment = crate::local_db::get_agent_review_comment(repo_path, comment_id)?
    .ok_or_else(|| format!("Review comment '{comment_id}' not found"))?;
  if !is_linear_target(&comment.target_type) {
    return Err(format!(
      "Review comment '{comment_id}' is not on Linear content"
    ));
  }
  let replacement = comment
    .suggested_replacement
    .as_deref()
    .ok_or_else(|| "Review comment has no suggested change".to_string())?;
  let quoted = comment
    .quoted_text
    .as_deref()
    .ok_or_else(|| "Review comment has no reviewed text to replace".to_string())?;
  let text = parse_text_ref(&comment.file_path)
    .ok_or_else(|| format!("Unknown Linear text '{}'", comment.file_path))?;
  let location = review_text_location(&comment.target_type, &comment.target_id, &text)?;

  let live = linear_get_review_text(client, &location).await?;
  let updated = splice_quoted(&live, quoted, replacement)?;
  linear_set_review_text(client, &location, &updated).await?;
  crate::local_db::resolve_agent_review_comment(repo_path, &comment.id)
}

fn poll_linear_kickoff(repo_path: &str) -> Result<(), String> {
  let db = crate::tracker::open_app_db()?;

  if !crate::core::feature_preview::is_enabled(
    &db,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  ) {
    return Ok(());
  }

  let label = db
    .get_repo_setting(repo_path, "linear_auto_kickoff_label")
    .map_err(|e| format!("Failed to read linear_auto_kickoff_label: {e}"))?;

  let label = match label {
    Some(l) if !l.is_empty() => l,
    _ => return Ok(()),
  };

  let client = resolve_linear_client(repo_path, &db)?;

  let rt =
    tokio::runtime::Runtime::new().map_err(|e| format!("Failed to create async runtime: {e}"))?;

  let labeled_ids = rt.block_on(linear_list_labeled_issue_ids_impl(&client, &label))?;

  let mut ledger = KickoffLedger::load(&db, repo_path, HANDLED_KEY, FAILURES_KEY)?;
  for issue_id in ledger.due(&labeled_ids) {
    match rt.block_on(kickoff_linear_issue_internal(repo_path, &client, &issue_id)) {
      Ok(_) => ledger.record_success(&issue_id),
      Err(e) => {
        let attempts = ledger.record_failure(&issue_id);
        log::warn!(
          "linear-kickoff: attempt {attempts}/{MAX_KICKOFF_ATTEMPTS} failed for {issue_id}: {e}"
        );
      }
    }
  }
  ledger.save(&db, repo_path, HANDLED_KEY, FAILURES_KEY)
}

const HANDLED_KEY: &str = "linear_handled_issue_ids";
const FAILURES_KEY: &str = "linear_kickoff_failures";

async fn kickoff_linear_issue_internal(
  repo_path: &str,
  client: &LinearClientSource,
  issue_id: &str,
) -> Result<crate::commands::linear::LinearKickoffResult, String> {
  let issue = linear_get_issue_impl(client, issue_id).await?;
  crate::commands::linear::open_or_create_workspace_from_linear_issue(repo_path, &issue).await
}

static GLOBAL_KICKOFF_POLLER: std::sync::OnceLock<KickoffPoller> = std::sync::OnceLock::new();

pub fn kickoff_poller() -> &'static KickoffPoller {
  GLOBAL_KICKOFF_POLLER.get_or_init(|| KickoffPoller::new("linear", poll_linear_kickoff))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn deserialize_linear_issue_response() {
    let json = serde_json::json!({
      "id": "ENG-1",
      "identifier": "ENG-1",
      "title": "Test Issue",
      "description": "Test description",
      "state": { "name": "In Progress", "type": "started" },
      "labels": { "nodes": [{ "name": "bug" }] },
      "branchName": "eng-1-test",
      "parent": null,
      "children": { "nodes": [] },
      "url": "https://linear.app/issue/ENG-1"
    });

    let node: LinearIssueNode = serde_json::from_value(json).unwrap();
    assert_eq!(node.identifier, "ENG-1");
    assert_eq!(node.title, "Test Issue");
    assert_eq!(node.state.name, "In Progress");
    assert_eq!(node.state.state_type, "started");
    assert_eq!(node.labels.nodes.len(), 1);
    assert_eq!(node.labels.nodes[0].name, "bug");
    assert_eq!(node.branch_name, "eng-1-test");
  }

  #[test]
  fn deserialize_linear_issue_with_subissues() {
    let json = serde_json::json!({
      "id": "ENG-1",
      "identifier": "ENG-1",
      "title": "Parent Issue",
      "description": null,
      "state": { "name": "Backlog", "type": "backlog" },
      "labels": { "nodes": [] },
      "branchName": "eng-1",
      "parent": null,
      "children": { "nodes": [{ "id": "ENG-2" }, { "id": "ENG-3" }] },
      "url": "https://linear.app/issue/ENG-1"
    });

    let node: LinearIssueNode = serde_json::from_value(json).unwrap();
    assert_eq!(node.sub_issues.nodes.len(), 2);
    assert_eq!(node.sub_issues.nodes[0].id, "ENG-2");
    assert_eq!(node.sub_issues.nodes[1].id, "ENG-3");
  }

  #[test]
  fn deserialize_linear_issue_response_with_parent() {
    let json = serde_json::json!({
      "id": "ENG-2",
      "identifier": "ENG-2",
      "title": "Child Issue",
      "description": null,
      "state": { "name": "Todo", "type": "backlog" },
      "labels": { "nodes": [] },
      "branchName": "eng-2",
      "parent": { "id": "ENG-1" },
      "children": { "nodes": [] },
      "url": "https://linear.app/issue/ENG-2"
    });

    let node: LinearIssueNode = serde_json::from_value(json).unwrap();
    assert_eq!(node.parent_id.unwrap().id, "ENG-1");
  }

  // Spliced into query text, this would close the string literal and inject syntax.
  const HOSTILE: &str = r#"ENG"}) { nodes { id } } #"#;

  #[test]
  fn list_issues_body_passes_team_as_variable() {
    let filter = team_issue_filter(Some(HOSTILE));
    let body = list_issues_body(filter.as_ref(), None);
    assert!(!body["query"].as_str().unwrap().contains(HOSTILE));
    assert_eq!(body["variables"]["filter"]["team"]["key"]["eq"], HOSTILE);
  }

  #[test]
  fn list_issues_body_sends_null_filter_when_unset() {
    let body = list_issues_body(team_issue_filter(None).as_ref(), None);
    assert!(body["variables"]["filter"].is_null());
    assert!(body["variables"]["after"].is_null());
  }

  #[test]
  fn list_issues_body_passes_cursor_as_variable() {
    let body = list_issues_body(None, Some(HOSTILE));
    assert!(!body["query"].as_str().unwrap().contains(HOSTILE));
    assert_eq!(body["variables"]["after"], HOSTILE);
    assert!(body["query"]
      .as_str()
      .unwrap()
      .contains("pageInfo { hasNextPage endCursor }"));
  }

  #[test]
  fn kickoff_filter_matches_label_and_skips_closed_issues() {
    let body = issue_page_body("id", Some(&kickoff_issue_filter(HOSTILE)), None);
    let query = body["query"].as_str().unwrap();
    assert!(!query.contains(HOSTILE));
    assert!(query.contains("nodes { id }"));
    let filter = &body["variables"]["filter"];
    assert_eq!(filter["labels"]["some"]["name"]["eqIgnoreCase"], HOSTILE);
    assert_eq!(
      filter["state"]["type"]["nin"],
      serde_json::json!(["completed", "canceled"])
    );
  }

  #[test]
  fn next_cursor_stops_on_last_page() {
    let more = LinearPageInfo {
      has_next_page: true,
      end_cursor: Some("c1".into()),
    };
    assert_eq!(next_cursor(more).as_deref(), Some("c1"));
    let last = LinearPageInfo {
      has_next_page: false,
      end_cursor: Some("c2".into()),
    };
    assert_eq!(next_cursor(last), None);
    // A missing cursor must stop paging rather than refetch page one forever.
    let broken = LinearPageInfo {
      has_next_page: true,
      end_cursor: None,
    };
    assert_eq!(next_cursor(broken), None);
  }

  #[test]
  fn get_issue_body_passes_id_as_variable() {
    let body = get_issue_body(HOSTILE);
    assert!(!body["query"].as_str().unwrap().contains(HOSTILE));
    assert_eq!(body["variables"]["id"], HOSTILE);
  }

  #[test]
  fn project_documents_body_passes_id_as_variable() {
    let body = project_documents_body(HOSTILE);
    assert!(!body["query"].as_str().unwrap().contains(HOSTILE));
    assert_eq!(body["variables"]["id"], HOSTILE);
  }

  #[test]
  fn entity_comments_body_passes_id_as_variable() {
    let body = entity_comments_body(CommentEntity::Document, HOSTILE);
    let query = body["query"].as_str().unwrap();
    assert!(!query.contains(HOSTILE));
    assert!(query.contains("document(id: $id)"));
    assert_eq!(body["variables"]["id"], HOSTILE);
  }

  #[derive(Deserialize, Debug)]
  struct ViewerOnly {
    viewer: TestUser,
  }

  #[derive(Deserialize, Debug)]
  struct TestUser {
    id: String,
  }

  #[test]
  fn graphql_response_returns_data_on_success() {
    let data: ViewerOnly = parse_graphql_response(
      reqwest::StatusCode::OK,
      r#"{"data":{"viewer":{"id":"u1","name":"Ada"}}}"#,
      false,
    )
    .unwrap();
    assert_eq!(data.viewer.id, "u1");
  }

  #[test]
  fn graphql_errors_win_over_http_status() {
    let err = parse_graphql_response::<ViewerOnly>(
      reqwest::StatusCode::BAD_REQUEST,
      r#"{"errors":[{"message":"Argument Validation Error"}]}"#,
      false,
    )
    .unwrap_err();
    assert_eq!(err, "Linear API error: Argument Validation Error");
  }

  #[test]
  fn non_json_error_body_reports_http_status() {
    let err = parse_graphql_response::<ViewerOnly>(
      reqwest::StatusCode::UNAUTHORIZED,
      "<html>Unauthorized</html>",
      false,
    )
    .unwrap_err();
    assert!(err.contains("rejected the API key"), "{err}");

    let err = parse_graphql_response::<ViewerOnly>(
      reqwest::StatusCode::TOO_MANY_REQUESTS,
      "slow down",
      false,
    )
    .unwrap_err();
    assert!(err.contains("rate limit"), "{err}");
  }

  #[test]
  fn error_status_without_graphql_errors_is_not_success() {
    let err = parse_graphql_response::<ViewerOnly>(
      reqwest::StatusCode::BAD_GATEWAY,
      r#"{"data":null}"#,
      false,
    )
    .unwrap_err();
    assert!(err.contains("502"), "{err}");
  }

  #[test]
  fn proxy_error_field_is_reported() {
    let err = parse_graphql_response::<ViewerOnly>(
      reqwest::StatusCode::FORBIDDEN,
      r#"{"error":"Linear account not linked"}"#,
      true,
    )
    .unwrap_err();
    assert_eq!(err, "Linear: Linear account not linked");

    let err = parse_graphql_response::<ViewerOnly>(
      reqwest::StatusCode::UNAUTHORIZED,
      r#"{"msg":"Invalid JWT"}"#,
      true,
    )
    .unwrap_err();
    assert!(err.contains("Sign in to treq again"), "{err}");
  }

  fn session() -> LinearProxySession {
    LinearProxySession {
      supabase_url: "https://proj.supabase.co".into(),
      access_token: "jwt".into(),
    }
  }

  #[test]
  fn api_key_wins_over_proxy_session() {
    let source = resolve_client_source(Some("lin_api".into()), Some(session())).unwrap();
    assert!(matches!(source, LinearClientSource::ApiKey(key) if key == "lin_api"));
  }

  #[test]
  fn proxy_session_is_used_without_api_key() {
    let source = resolve_client_source(None, Some(session())).unwrap();
    assert!(matches!(source, LinearClientSource::Proxy(s) if s.access_token == "jwt"));
  }

  #[test]
  fn no_credentials_is_a_clear_error() {
    let err = resolve_client_source(None, None).err().unwrap();
    assert!(err.contains("not connected"), "{err}");
  }

  #[test]
  fn set_proxy_session_normalizes_and_clears() {
    set_proxy_session(
      Some("https://proj.supabase.co/ ".into()),
      Some("jwt".into()),
    );
    assert_eq!(
      proxy_session().map(|s| s.supabase_url).as_deref(),
      Some("https://proj.supabase.co")
    );
    set_proxy_session(Some("https://proj.supabase.co".into()), None);
    assert!(proxy_session().is_none());
  }

  fn api_key(key: &str) -> LinearClientSource {
    LinearClientSource::ApiKey(key.to_string())
  }

  struct TestEndpointGuard;

  impl Drop for TestEndpointGuard {
    fn drop(&mut self) {
      TEST_ENDPOINT.with(|e| *e.borrow_mut() = None);
    }
  }

  fn use_mock_endpoint(server: &wiremock::MockServer, timeout: Duration) -> TestEndpointGuard {
    TEST_ENDPOINT.with(|e| *e.borrow_mut() = Some((server.uri(), timeout)));
    TestEndpointGuard
  }

  #[test]
  fn review_text_bodies_pass_ids_and_text_as_variables() {
    use crate::core::linear_review::LinearTextRef;
    let location = review_text_location("linear_document", HOSTILE, &LinearTextRef::Body).unwrap();
    let get = get_review_text_body(&location);
    assert!(!get["query"].as_str().unwrap().contains(HOSTILE));
    assert!(get["query"]
      .as_str()
      .unwrap()
      .contains("document(id: $id) { content }"));
    let set = set_review_text_body(&location, HOSTILE);
    let query = set["query"].as_str().unwrap();
    assert!(!query.contains(HOSTILE));
    assert!(query.contains("documentUpdate(id: $id, input: { content: $text })"));
    assert_eq!(set["variables"]["text"], HOSTILE);

    let comment = review_text_location(
      "linear_issue",
      "issue-1",
      &LinearTextRef::Comment("c-1".into()),
    )
    .unwrap();
    assert_eq!(
      (comment.root, comment.mutation),
      ("comment", "commentUpdate")
    );
    assert_eq!(comment.id, "c-1");
    assert!(review_text_location("workspace_diff", "1", &LinearTextRef::Body).is_err());
  }

  #[tokio::test]
  async fn applying_a_review_suggestion_rewrites_the_linear_text() {
    use wiremock::{
      matchers::{body_partial_json, method},
      Mock, MockServer, ResponseTemplate,
    };
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    let comment = crate::local_db::create_agent_review_comment(
      repo,
      "linear_issue",
      "issue-1",
      "body.md",
      None,
      2,
      2,
      None,
      "Typo",
      Some("Ship the fix"),
      "local-agent",
      Some("Ship teh fix"),
    )
    .unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(body_partial_json(
        serde_json::json!({ "variables": { "id": "issue-1" } }),
      ))
      .and(body_partial_json(
        serde_json::json!({ "variables": { "text": "Intro\nShip the fix\nEnd" } }),
      ))
      .respond_with(
        ResponseTemplate::new(200)
          .set_body_json(serde_json::json!({ "data": { "issueUpdate": { "success": true } } })),
      )
      .expect(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "data": { "issue": { "description": "Intro\nShip teh fix\nEnd" } }
      })))
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    linear_apply_review_suggestion_impl(&api_key("lin_api_test"), repo, &comment.id)
      .await
      .unwrap();
    let stored = crate::local_db::get_agent_review_comment(repo, &comment.id)
      .unwrap()
      .unwrap();
    assert_eq!(stored.status, "resolved");
  }

  #[tokio::test]
  async fn applying_a_suggestion_to_changed_text_fails_without_writing() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    let comment = crate::local_db::create_agent_review_comment(
      repo,
      "linear_project",
      "p-1",
      "body.md",
      None,
      1,
      1,
      None,
      "Typo",
      Some("new"),
      "local-agent",
      Some("old"),
    )
    .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "data": { "project": { "description": "rewritten by someone" } }
      })))
      .expect(1)
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let err = linear_apply_review_suggestion_impl(&api_key("lin_api_test"), repo, &comment.id)
      .await
      .unwrap_err();
    assert!(err.contains("changed after the review"), "{err}");
    let stored = crate::local_db::get_agent_review_comment(repo, &comment.id)
      .unwrap()
      .unwrap();
    assert_eq!(stored.status, "open");
  }

  #[tokio::test]
  async fn stalled_linear_request_times_out() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(
        ResponseTemplate::new(200)
          .set_body_string(r#"{"data":{"viewer":{"id":"u1","name":"Ada"}}}"#)
          .set_delay(Duration::from_secs(5)),
      )
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_millis(200));

    let result = tokio::time::timeout(
      Duration::from_secs(3),
      linear_get_viewer_impl(&api_key("lin_api_test")),
    )
    .await
    .expect("the request must fail on its own timeout, not hang");
    let err = result.expect_err("a stalled request is an error");
    assert!(err.contains("timed out"), "{err}");
  }

  #[tokio::test]
  async fn unauthorized_html_response_reports_the_api_key() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(401).set_body_string("<html>Unauthorized</html>"))
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let err = linear_get_viewer_impl(&api_key("lin_api_bad"))
      .await
      .unwrap_err();
    assert!(err.contains("rejected the API key"), "{err}");
  }

  #[tokio::test]
  async fn api_key_is_sent_bare_in_authorization_header() {
    use wiremock::{
      matchers::{header, method},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(header("authorization", "lin_api_test"))
      .respond_with(
        ResponseTemplate::new(200)
          .set_body_string(r#"{"data":{"viewer":{"id":"u1","name":"Ada"}}}"#),
      )
      .expect(1)
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let viewer = linear_get_viewer_impl(&api_key("lin_api_test"))
      .await
      .unwrap();
    assert_eq!(viewer.id, "u1");
  }

  fn issue_page(ids: &[&str], next: Option<&str>) -> serde_json::Value {
    let nodes: Vec<_> = ids
      .iter()
      .map(|id| {
        serde_json::json!({
          "id": id,
          "identifier": id.to_uppercase(),
          "title": format!("Issue {id}"),
          "branchName": format!("branch-{id}"),
          "url": format!("https://linear.app/t/issue/{id}"),
        })
      })
      .collect();
    serde_json::json!({
      "data": { "issues": {
        "nodes": nodes,
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
      } }
    })
  }

  #[tokio::test]
  async fn list_issues_follows_cursors_past_the_first_page() {
    use wiremock::{
      matchers::{body_partial_json, method},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(body_partial_json(
        serde_json::json!({ "variables": { "after": "c1" } }),
      ))
      .respond_with(ResponseTemplate::new(200).set_body_json(issue_page(&["b"], None)))
      .with_priority(1)
      .expect(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(issue_page(&["a"], Some("c1"))))
      .with_priority(2)
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let issues = linear_list_issues_impl(&api_key("lin_api_test"), Some("ENG"))
      .await
      .unwrap();
    let ids: Vec<_> = issues.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
  }

  #[tokio::test]
  async fn list_issues_stops_at_the_page_cap() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(issue_page(&["x"], Some("again"))))
      .expect(MAX_ISSUE_PAGES as u64)
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let issues = linear_list_issues_impl(&api_key("lin_api_test"), None)
      .await
      .unwrap();
    assert_eq!(issues.len(), MAX_ISSUE_PAGES);
  }

  fn labeled_node(id: &str, label: &str, state_type: &str) -> serde_json::Value {
    serde_json::json!({
      "id": id,
      "identifier": id.to_uppercase(),
      "title": format!("Issue {id}"),
      "branchName": format!("branch-{id}"),
      "url": format!("https://linear.app/t/issue/{id}"),
      "state": { "name": state_type, "type": state_type },
      "labels": { "nodes": [{ "name": label }] },
    })
  }

  fn nodes_page(nodes: Vec<serde_json::Value>, next: Option<&str>) -> serde_json::Value {
    serde_json::json!({
      "data": { "issues": {
        "nodes": nodes,
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
      } }
    })
  }

  fn kickoff_filter_matcher(label: &str) -> wiremock::matchers::BodyPartialJsonMatcher {
    wiremock::matchers::body_partial_json(serde_json::json!({
      "variables": { "filter": kickoff_issue_filter(label) }
    }))
  }

  #[tokio::test]
  async fn kickoff_finds_labeled_issues_outside_the_first_unfiltered_page() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    // Linear only returns the labeled issue when asked for it; an
    // unfiltered first page is full of other issues.
    Mock::given(method("POST"))
      .and(kickoff_filter_matcher("agent"))
      .respond_with(ResponseTemplate::new(200).set_body_json(nodes_page(
        vec![labeled_node("old-labeled", "agent", "unstarted")],
        None,
      )))
      .with_priority(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(nodes_page(
        vec![labeled_node("recent", "bug", "started")],
        Some("more"),
      )))
      .with_priority(2)
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let ids = linear_list_labeled_issue_ids_impl(&api_key("lin_api_test"), "agent")
      .await
      .unwrap();
    assert_eq!(ids, ["old-labeled"]);
  }

  #[tokio::test]
  async fn kickoff_skips_closed_labeled_issues() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(kickoff_filter_matcher("agent"))
      .respond_with(ResponseTemplate::new(200).set_body_json(nodes_page(vec![], None)))
      .with_priority(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(nodes_page(
        vec![
          labeled_node("done", "agent", "completed"),
          labeled_node("dropped", "agent", "canceled"),
        ],
        None,
      )))
      .with_priority(2)
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let ids = linear_list_labeled_issue_ids_impl(&api_key("lin_api_test"), "agent")
      .await
      .unwrap();
    assert!(ids.is_empty(), "{ids:?}");
  }

  #[tokio::test]
  async fn kickoff_refuses_a_partial_list_instead_of_truncating() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(200).set_body_json(nodes_page(
        vec![labeled_node("x", "agent", "unstarted")],
        Some("again"),
      )))
      .mount(&server)
      .await;
    let _guard = use_mock_endpoint(&server, Duration::from_secs(5));

    let result = linear_list_labeled_issue_ids_impl(&api_key("lin_api_test"), "agent").await;
    assert!(result.is_err(), "{result:?}");
  }

  #[tokio::test]
  async fn oauth_session_reaches_linear_through_the_proxy() {
    use wiremock::{
      matchers::{header, method, path},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/linear-proxy"))
      .and(header("authorization", "Bearer supabase-jwt"))
      .respond_with(
        ResponseTemplate::new(200)
          .set_body_string(r#"{"data":{"viewer":{"id":"u1","name":"Ada"}}}"#),
      )
      .expect(1)
      .mount(&server)
      .await;
    let session = LinearProxySession {
      supabase_url: server.uri(),
      access_token: "supabase-jwt".into(),
    };

    let client = resolve_client_source(None, Some(session)).unwrap();
    let viewer = linear_get_viewer_impl(&client).await.unwrap();
    assert_eq!(viewer.id, "u1");
  }

  #[tokio::test]
  async fn proxy_explains_an_unlinked_linear_account() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(
        ResponseTemplate::new(403).set_body_string(r#"{"error":"Linear account not linked"}"#),
      )
      .mount(&server)
      .await;
    let client = LinearClientSource::Proxy(LinearProxySession {
      supabase_url: server.uri(),
      access_token: "supabase-jwt".into(),
    });

    let err = linear_get_viewer_impl(&client).await.unwrap_err();
    assert_eq!(err, "Linear: Linear account not linked");
  }
}
