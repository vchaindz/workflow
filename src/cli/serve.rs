use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use crate::core::config::Config;
use crate::core::db;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::core::executor::{execute_workflow, ExecuteOpts};
use crate::core::models::TaskKind;
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

#[derive(Clone)]
struct RunState {
    status: RunStatus,
    run_log: Option<crate::core::models::RunLog>,
    error: Option<String>,
    created_at: std::time::Instant,
}

#[derive(Clone, PartialEq)]
enum RunStatus {
    Running,
    Completed,
    Failed,
}

/// Maximum number of tracked runs before eviction is forced.
const MAX_TRACKED_RUNS: usize = 1000;

/// Maximum request body size in bytes (1 MB).
const MAX_BODY_SIZE: u64 = 1_048_576;

/// How long to keep completed/failed runs before eviction.
const RUN_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

pub fn cmd_serve(config: &Config, port: u16, bind: &str) -> Result<i32> {
    let addr = format!("{bind}:{port}");
    let server = tiny_http::Server::http(&addr).map_err(|e| {
        crate::error::DzError::Execution(format!("failed to start server on {addr}: {e}"))
    })?;

    // Auto-generate API key if not configured
    let api_key = match &config.server.api_key {
        Some(key) => key.clone(),
        None => {
            let generated = uuid::Uuid::new_v4().to_string();
            eprintln!("No API key configured — auto-generated key: {generated}");
            eprintln!("Set [server] api_key in config.toml to use a persistent key.");
            generated
        }
    };

    eprintln!("workflow server listening on http://{addr}");
    eprintln!("  POST /run/<category>/<task>  — trigger a workflow");
    eprintln!("  GET  /status/<run_id>        — poll run status");
    eprintln!("  GET  /tasks                  — list available tasks");
    eprintln!("  GET  /health                 — health check");

    let runs: Arc<Mutex<HashMap<String, RunState>>> = Arc::new(Mutex::new(HashMap::new()));
    let active_count = Arc::new(AtomicUsize::new(0));
    let max_concurrent = config.server.max_concurrent_runs;
    let workflows_dir = config.workflows_dir.clone();
    let db_path = config.db_path();

    loop {
        let mut request = match server.recv() {
            Ok(req) => req,
            Err(e) => {
                eprintln!("server recv error: {e}");
                continue;
            }
        };

        // Auth check — always enforced
        {
            let auth_header = request
                .headers()
                .iter()
                .find(|h| h.field.as_str().to_ascii_lowercase() == "authorization")
                .map(|h| h.value.as_str().to_string());

            let expected = format!("Bearer {api_key}");
            match auth_header {
                Some(ref v) if v == &expected => {}
                _ => {
                    let _ = request.respond(json_response(401, r#"{"error":"unauthorized"}"#));
                    continue;
                }
            }
        }

        let method = request.method().as_str().to_uppercase();
        let url = request.url().to_string();

        // CSRF mitigation: non-GET requests must include X-Workflow-Client header.
        // Browsers won't send custom headers on cross-origin requests without CORS preflight.
        if method != "GET" {
            let has_csrf_header = request
                .headers()
                .iter()
                .any(|h| h.field.as_str().to_ascii_lowercase() == "x-workflow-client");
            if !has_csrf_header {
                let _ = request.respond(json_response(
                    403,
                    r#"{"error":"missing X-Workflow-Client header"}"#,
                ));
                continue;
            }
        }

        // Strip query string before routing
        let path = url.split('?').next().unwrap_or(&url);
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        match (method.as_str(), segments.as_slice()) {
            ("GET", ["health"]) => {
                let _ = request.respond(json_response(200, r#"{"status":"ok"}"#));
            }

            ("GET", ["tasks"]) => {
                match scan_workflows(&workflows_dir) {
                    Ok(categories) => {
                        let tasks: Vec<serde_json::Value> = categories
                            .iter()
                            .flat_map(|c| {
                                c.tasks.iter().map(move |t| {
                                    serde_json::json!({
                                        "ref": format!("{}/{}", c.name, t.name),
                                        "kind": format!("{:?}", t.kind),
                                        "path": t.path.display().to_string(),
                                    })
                                })
                            })
                            .collect();
                        let body = serde_json::json!({ "tasks": tasks }).to_string();
                        let _ = request.respond(json_response(200, &body));
                    }
                    Err(e) => {
                        let body = serde_json::json!({ "error": e.to_string() }).to_string();
                        let _ = request.respond(json_response(500, &body));
                    }
                }
            }

            ("GET", ["status", run_id]) => {
                let runs_lock = runs.lock().unwrap();
                match runs_lock.get(*run_id) {
                    Some(state) => {
                        let status_str = match state.status {
                            RunStatus::Running => "running",
                            RunStatus::Completed => "completed",
                            RunStatus::Failed => "failed",
                        };
                        let mut body =
                            serde_json::json!({ "run_id": run_id, "status": status_str });
                        if let Some(ref log) = state.run_log {
                            body["exit_code"] = serde_json::json!(log.exit_code);
                            body["steps"] = serde_json::json!(log.steps.len());
                            body["started"] = serde_json::json!(log.started.to_rfc3339());
                            if let Some(ended) = log.ended {
                                body["ended"] = serde_json::json!(ended.to_rfc3339());
                            }
                        }
                        if let Some(ref err) = state.error {
                            body["error"] = serde_json::json!(err);
                        }
                        let _ = request.respond(json_response(200, &body.to_string()));
                    }
                    None => {
                        let body = serde_json::json!({ "error": "run not found" }).to_string();
                        let _ = request.respond(json_response(404, &body));
                    }
                }
            }

            ("POST", ["run", category, task]) => {
                let task_ref = format!("{category}/{task}");

                // Check concurrent limit
                if active_count.load(Ordering::Relaxed) >= max_concurrent {
                    let body = serde_json::json!({
                        "error": "too many concurrent runs",
                        "max": max_concurrent,
                    })
                    .to_string();
                    let _ = request.respond(json_response(429, &body));
                    continue;
                }

                // Check Content-Length before reading body
                let content_length = request
                    .headers()
                    .iter()
                    .find(|h| h.field.as_str().to_ascii_lowercase() == "content-length")
                    .and_then(|h| h.value.as_str().parse::<u64>().ok());

                if let Some(len) = content_length {
                    if len > MAX_BODY_SIZE {
                        let body = serde_json::json!({
                            "error": "request body too large",
                            "max_bytes": MAX_BODY_SIZE,
                        })
                        .to_string();
                        let _ = request.respond(json_response(413, &body));
                        continue;
                    }
                }

                // Read body with size limit
                let mut body_buf = vec![0u8; (MAX_BODY_SIZE + 1) as usize];
                let mut total_read = 0usize;
                let reader = request.as_reader();
                loop {
                    if total_read >= body_buf.len() {
                        break;
                    }
                    match std::io::Read::read(reader, &mut body_buf[total_read..]) {
                        Ok(0) => break,
                        Ok(n) => total_read += n,
                        Err(_) => break,
                    }
                }

                if total_read as u64 > MAX_BODY_SIZE {
                    let body = serde_json::json!({
                        "error": "request body too large",
                        "max_bytes": MAX_BODY_SIZE,
                    })
                    .to_string();
                    let _ = request.respond(json_response(413, &body));
                    continue;
                }

                let body_str = String::from_utf8_lossy(&body_buf[..total_read]).into_owned();

                let env_overrides: HashMap<String, String> = if body_str.is_empty() {
                    HashMap::new()
                } else {
                    match serde_json::from_str::<serde_json::Value>(&body_str) {
                        Ok(val) => {
                            if let Some(env_obj) = val.get("env").and_then(|e| e.as_object()) {
                                env_obj
                                    .iter()
                                    .filter_map(|(k, v)| {
                                        v.as_str().map(|s| (k.clone(), s.to_string()))
                                    })
                                    .collect()
                            } else {
                                HashMap::new()
                            }
                        }
                        Err(_) => HashMap::new(),
                    }
                };

                // Resolve workflow
                let categories = match scan_workflows(&workflows_dir) {
                    Ok(c) => c,
                    Err(e) => {
                        let body = serde_json::json!({ "error": e.to_string() }).to_string();
                        let _ = request.respond(json_response(500, &body));
                        continue;
                    }
                };

                let resolved = match resolve_task_ref(&categories, &task_ref) {
                    Ok(t) => t.clone(),
                    Err(e) => {
                        let body = serde_json::json!({ "error": e.to_string() }).to_string();
                        let _ = request.respond(json_response(404, &body));
                        continue;
                    }
                };

                let run_id = uuid::Uuid::new_v4().to_string();

                // Evict old completed/failed runs before inserting
                {
                    let mut runs_lock = runs.lock().unwrap();
                    let now = std::time::Instant::now();

                    // Time-based eviction: remove completed/failed runs older than TTL
                    runs_lock.retain(|_, state| {
                        state.status == RunStatus::Running
                            || now.duration_since(state.created_at) < RUN_TTL
                    });

                    // Hard cap: if still over limit, remove oldest completed/failed
                    if runs_lock.len() >= MAX_TRACKED_RUNS {
                        let mut finished: Vec<(String, std::time::Instant)> = runs_lock
                            .iter()
                            .filter(|(_, s)| s.status != RunStatus::Running)
                            .map(|(k, s)| (k.clone(), s.created_at))
                            .collect();
                        finished.sort_by_key(|(_, t)| *t);
                        let to_remove = finished.len().min(runs_lock.len() - MAX_TRACKED_RUNS + 1);
                        for (key, _) in finished.into_iter().take(to_remove) {
                            runs_lock.remove(&key);
                        }
                    }

                    runs_lock.insert(
                        run_id.clone(),
                        RunState {
                            status: RunStatus::Running,
                            run_log: None,
                            error: None,
                            created_at: now,
                        },
                    );
                }

                // Respond 202 immediately
                let body = serde_json::json!({
                    "run_id": run_id,
                    "task_ref": task_ref,
                    "status": "accepted",
                })
                .to_string();
                let _ = request.respond(json_response(202, &body));

                // Spawn execution thread
                let runs_clone = runs.clone();
                let active_clone = active_count.clone();
                let run_id_clone = run_id;
                let task_ref_clone = task_ref;
                let db_path_clone = db_path.clone();
                let wf_dir_clone = workflows_dir.clone();

                active_clone.fetch_add(1, Ordering::Relaxed);

                std::thread::spawn(move || {
                    let result = (|| -> std::result::Result<crate::core::models::RunLog, String> {
                        let workflow = match resolved.kind {
                            TaskKind::ShellScript => {
                                parse_shell_task(&resolved.path).map_err(|e| e.to_string())?
                            }
                            TaskKind::YamlWorkflow => {
                                parse_workflow(&resolved.path).map_err(|e| e.to_string())?
                            }
                        };

                        let opts = ExecuteOpts {
                            dry_run: false,
                            force: false,
                            env_overrides,
                            default_timeout: None,
                            secrets: workflow.secrets.clone(),
                            interactive_tx: None,
                            streaming_tx: None,
                            workflows_dir: Some(wf_dir_clone),
                            call_depth: 0,
                            max_call_depth: 10,
                            secrets_ssh_key: None,
                            mcp_servers: std::collections::HashMap::new(),
                        };

                        let run_log = execute_workflow(&workflow, &task_ref_clone, &opts, None)
                            .map_err(|e| e.to_string())?;

                        // Log to database
                        if let Ok(conn) = db::open_db(&db_path_clone) {
                            let _ = db::insert_run_log_with_source(&conn, &run_log, "api");
                        }

                        Ok(run_log)
                    })();

                    let mut runs_lock = runs_clone.lock().unwrap();
                    let created_at = runs_lock
                        .get(&run_id_clone)
                        .map(|s| s.created_at)
                        .unwrap_or_else(std::time::Instant::now);

                    match result {
                        Ok(run_log) => {
                            let status = if run_log.exit_code == 0 {
                                RunStatus::Completed
                            } else {
                                RunStatus::Failed
                            };
                            runs_lock.insert(
                                run_id_clone,
                                RunState {
                                    status,
                                    run_log: Some(run_log),
                                    error: None,
                                    created_at,
                                },
                            );
                        }
                        Err(e) => {
                            runs_lock.insert(
                                run_id_clone,
                                RunState {
                                    status: RunStatus::Failed,
                                    run_log: None,
                                    error: Some(e),
                                    created_at,
                                },
                            );
                        }
                    }

                    active_clone.fetch_sub(1, Ordering::Relaxed);
                });
            }

            _ => {
                let body = serde_json::json!({ "error": "not found" }).to_string();
                let _ = request.respond(json_response(404, &body));
            }
        }
    }
}

fn json_response(status: u16, body: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        )
}
