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
}

#[derive(Clone, PartialEq)]
enum RunStatus {
    Running,
    Completed,
    Failed,
}

pub fn cmd_serve(config: &Config, port: u16, bind: &str) -> Result<i32> {
    let addr = format!("{bind}:{port}");
    let server = tiny_http::Server::http(&addr).map_err(|e| {
        crate::error::DzError::Execution(format!("failed to start server on {addr}: {e}"))
    })?;

    eprintln!("workflow server listening on http://{addr}");
    eprintln!("  POST /run/<category>/<task>  — trigger a workflow");
    eprintln!("  GET  /status/<run_id>        — poll run status");
    eprintln!("  GET  /tasks                  — list available tasks");
    eprintln!("  GET  /health                 — health check");

    let runs: Arc<Mutex<HashMap<String, RunState>>> = Arc::new(Mutex::new(HashMap::new()));
    let active_count = Arc::new(AtomicUsize::new(0));
    let max_concurrent = config.server.max_concurrent_runs;
    let api_key = config.server.api_key.clone();
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

        // Auth check
        if let Some(ref key) = api_key {
            let auth_header = request
                .headers()
                .iter()
                .find(|h| h.field.as_str().to_ascii_lowercase() == "authorization")
                .map(|h| h.value.as_str().to_string());

            let expected = format!("Bearer {key}");
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

                // Parse request body for env vars
                let mut body_str = String::new();
                let mut reader = request.as_reader();
                let _ = std::io::Read::read_to_string(&mut reader, &mut body_str);

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

                // Register the run
                {
                    let mut runs_lock = runs.lock().unwrap();
                    runs_lock.insert(
                        run_id.clone(),
                        RunState {
                            status: RunStatus::Running,
                            run_log: None,
                            error: None,
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
