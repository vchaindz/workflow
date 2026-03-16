use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::core::models::{EnvValue, RawStep, RawWorkflow, Step, Workflow};
use crate::error::{DzError, Result};

/// Parse a YAML workflow from a string. Used by `parse_workflow()` and for
/// in-memory parsing (e.g. wizard dry-run preview).
pub fn parse_workflow_from_str(contents: &str) -> Result<Workflow> {
    let raw: RawWorkflow = serde_yaml::from_str(contents)?;
    let steps = normalize_steps(raw.steps)?;
    let cleanup = if raw.cleanup.is_empty() {
        Vec::new()
    } else {
        normalize_steps(raw.cleanup)?
    };
    let workflow = Workflow {
        name: raw.name,
        steps,
        env: raw.env,
        workdir: raw.workdir,
        secrets: raw.secrets,
        notify: raw.notify,
        overdue: raw.overdue,
        variables: raw.variables,
        cleanup,
    };

    validate_workflow(&workflow)?;
    Ok(workflow)
}

/// Parse a YAML workflow file into a Workflow struct.
/// Supports three step formats: bare strings, maps without id, and full maps.
pub fn parse_workflow(path: &Path) -> Result<Workflow> {
    let contents = std::fs::read_to_string(path)?;
    parse_workflow_from_str(&contents)
}

/// Convert raw flexible-format steps into canonical Steps with auto-IDs and sequential chaining.
fn normalize_steps(raw_steps: Vec<RawStep>) -> Result<Vec<Step>> {
    let mut steps = Vec::with_capacity(raw_steps.len());
    let mut prev_auto_id: Option<String> = None;
    let mut used_ids: HashSet<String> = HashSet::new();

    for (i, raw) in raw_steps.into_iter().enumerate() {
        let step = match raw {
            RawStep::CmdString(cmd) => {
                let id = format!("step-{}", i + 1);
                let needs = prev_auto_id.take().into_iter().collect();
                prev_auto_id = Some(id.clone());
                Step { id, cmd, needs, parallel: false, timeout: None, run_if: None, skip_if: None, retry: None, retry_delay: None, interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None }
            }
            RawStep::CmdMap { id: None, cmd, needs: _, parallel, timeout, run_if, skip_if, retry, retry_delay, interactive, outputs, call, for_each, for_each_cmd, for_each_parallel, for_each_continue_on_error, mcp } => {
                let id = format!("step-{}", i + 1);
                let needs = prev_auto_id.take().into_iter().collect();
                prev_auto_id = Some(id.clone());
                Step { id, cmd, needs, parallel, timeout, run_if, skip_if, retry, retry_delay, interactive, outputs, call, for_each: for_each.map(|b| *b), for_each_cmd, for_each_parallel, for_each_continue_on_error, mcp }
            }
            RawStep::CmdMap { id: Some(id), cmd, needs, parallel, timeout, run_if, skip_if, retry, retry_delay, interactive, outputs, call, for_each, for_each_cmd, for_each_parallel, for_each_continue_on_error, mcp } => {
                // Explicit id: no implicit chaining, but don't break the chain for others
                Step { id, cmd, needs, parallel, timeout, run_if, skip_if, retry, retry_delay, interactive, outputs, call, for_each: for_each.map(|b| *b), for_each_cmd, for_each_parallel, for_each_continue_on_error, mcp }
            }
        };

        if !used_ids.insert(step.id.clone()) {
            return Err(DzError::Parse(format!("duplicate step id '{}'", step.id)));
        }
        steps.push(step);
    }

    // Validate mutual exclusion of cmd, call, and mcp
    for step in &steps {
        let has_cmd = !step.cmd.is_empty();
        let has_call = step.call.is_some();
        let has_mcp = step.mcp.is_some();

        let set_count = has_cmd as u8 + has_call as u8 + has_mcp as u8;
        if set_count > 1 {
            let mut fields = Vec::new();
            if has_cmd { fields.push("cmd"); }
            if has_call { fields.push("call"); }
            if has_mcp { fields.push("mcp"); }
            return Err(DzError::Parse(format!(
                "step '{}' has both '{}' and '{}' — these are mutually exclusive",
                step.id, fields[0], fields[1]
            )));
        }

        // When mcp feature is disabled, reject mcp steps with a helpful error
        #[cfg(not(feature = "mcp"))]
        if has_mcp {
            return Err(DzError::Parse(format!(
                "step '{}' uses 'mcp' which requires the mcp feature. Rebuild with: cargo build --features mcp",
                step.id
            )));
        }

        // Validate mcp step has a tool field (enforced by deserialization, but belt-and-suspenders)
        if let Some(ref mcp) = step.mcp {
            if mcp.tool.is_empty() {
                return Err(DzError::Parse(format!(
                    "step '{}' has 'mcp' but no 'tool' specified",
                    step.id
                )));
            }
        }

        if step.for_each.is_some() && step.for_each_cmd.is_some() {
            return Err(DzError::Parse(format!(
                "step '{}' has both 'for_each' and 'for_each_cmd' — these are mutually exclusive",
                step.id
            )));
        }
    }

    Ok(steps)
}

/// Resolve env values: static strings pass through, dynamic `cmd` values are executed via bash.
/// Called at execution time only — never during parsing, validation, or preview.
pub fn resolve_env(raw_env: &HashMap<String, EnvValue>) -> Result<HashMap<String, String>> {
    let mut resolved = HashMap::new();
    for (key, val) in raw_env.iter() {
        match val {
            EnvValue::Static(s) => {
                resolved.insert(key.clone(), s.clone());
            }
            EnvValue::Dynamic { cmd } => {
                let output = std::process::Command::new("bash")
                    .arg("-c")
                    .arg(cmd)
                    .output()
                    .map_err(|e| DzError::Execution(format!("env '{key}': {e}")))?;
                if !output.status.success() {
                    return Err(DzError::Execution(format!(
                        "env '{key}' cmd failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    )));
                }
                resolved.insert(
                    key.clone(),
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                );
            }
        }
    }
    Ok(resolved)
}

/// Return only static env values (no command execution). For preview/cache/validate.
pub fn static_env(env: &HashMap<String, EnvValue>) -> HashMap<String, String> {
    env.iter()
        .filter_map(|(k, v)| match v {
            EnvValue::Static(s) => Some((k.clone(), s.clone())),
            EnvValue::Dynamic { .. } => None,
        })
        .collect()
}

/// Wrap a shell script as a single-step workflow.
pub fn parse_shell_task(path: &Path) -> Result<Workflow> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("script")
        .to_string();

    let cmd = format!("bash '{}'", path.display().to_string().replace('\'', "'\\''"));

    Ok(Workflow {
        name,
        workdir: None,
        steps: vec![Step {
            id: "run".to_string(),
            cmd,
            needs: Vec::new(),
            parallel: false,
            timeout: None,
            run_if: None,
            skip_if: None,
            retry: None,
            retry_delay: None,
            interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
        }],
        env: HashMap::new(),
        secrets: Vec::new(),
        notify: Default::default(),
        overdue: None,
        variables: Vec::new(),
        cleanup: Vec::new(),
    })
}

/// Validate step dependencies exist and detect cycles.
fn validate_workflow(workflow: &Workflow) -> Result<()> {
    let step_ids: HashSet<&str> = workflow.steps.iter().map(|s| s.id.as_str()).collect();

    // Check all dependencies reference existing steps
    for step in &workflow.steps {
        for dep in &step.needs {
            if !step_ids.contains(dep.as_str()) {
                return Err(DzError::Parse(format!(
                    "step '{}' depends on unknown step '{dep}'",
                    step.id
                )));
            }
        }
    }

    // Cycle detection via Kahn's algorithm (topological sort)
    detect_cycles(&workflow.steps)?;

    Ok(())
}

/// Topological sort using Kahn's algorithm. Returns error if cycle detected.
pub fn topological_sort(steps: &[Step]) -> Result<Vec<String>> {
    detect_cycles(steps)
}

fn detect_cycles(steps: &[Step]) -> Result<Vec<String>> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for step in steps {
        in_degree.entry(step.id.as_str()).or_insert(0);
        for dep in &step.needs {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(step.id.as_str());
            *in_degree.entry(step.id.as_str()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut sorted = Vec::new();

    while let Some(node) = queue.pop_front() {
        sorted.push(node.to_string());
        if let Some(deps) = dependents.get(node) {
            for &dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }
    }

    if sorted.len() != steps.len() {
        let remaining: Vec<String> = steps
            .iter()
            .filter(|s| !sorted.contains(&s.id))
            .map(|s| s.id.clone())
            .collect();
        return Err(DzError::CycleDetected(remaining));
    }

    Ok(sorted)
}

/// Compute execution levels for parallel execution.
/// Returns Vec<Vec<String>> where each inner Vec contains step IDs that can run concurrently.
/// Steps in level N+1 depend only on steps in levels 0..=N.
pub fn compute_execution_levels(steps: &[Step]) -> Result<Vec<Vec<String>>> {
    // First validate no cycles
    detect_cycles(steps)?;

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for step in steps {
        in_degree.entry(step.id.as_str()).or_insert(0);
        for dep in &step.needs {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(step.id.as_str());
            *in_degree.entry(step.id.as_str()).or_insert(0) += 1;
        }
    }

    let mut levels: Vec<Vec<String>> = Vec::new();
    let mut remaining: HashMap<&str, usize> = in_degree;

    loop {
        let level: Vec<String> = remaining
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id.to_string())
            .collect();

        if level.is_empty() {
            break;
        }

        for id in &level {
            if let Some(deps) = dependents.get(id.as_str()) {
                for &dep in deps {
                    if let Some(deg) = remaining.get_mut(dep) {
                        *deg -= 1;
                    }
                }
            }
        }

        for id in &level {
            remaining.remove(id.as_str());
        }

        levels.push(level);
    }

    Ok(levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_yaml_workflow() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(
            &path,
            r#"
name: Test Workflow
steps:
  - id: step1
    cmd: echo hello
  - id: step2
    cmd: echo world
    needs: [step1]
env:
  FOO: bar
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.name, "Test Workflow");
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[1].needs, vec!["step1"]);
        match wf.env.get("FOO").unwrap() {
            EnvValue::Static(s) => assert_eq!(s, "bar"),
            _ => panic!("expected static env"),
        }
    }

    #[test]
    fn test_parse_shell_task() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backup.sh");
        fs::write(&path, "#!/bin/bash\necho backup").unwrap();

        let wf = parse_shell_task(&path).unwrap();
        assert_eq!(wf.name, "backup");
        assert_eq!(wf.steps.len(), 1);
        assert!(wf.steps[0].cmd.contains("bash"));
    }

    #[test]
    fn test_cycle_detection() {
        let steps = vec![
            Step {
                id: "a".into(),
                cmd: "echo a".into(),
                needs: vec!["c".into()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "b".into(),
                cmd: "echo b".into(),
                needs: vec!["a".into()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "c".into(),
                cmd: "echo c".into(),
                needs: vec!["b".into()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
        ];

        let result = detect_cycles(&steps);
        assert!(result.is_err());
        match result.unwrap_err() {
            DzError::CycleDetected(ids) => {
                assert_eq!(ids.len(), 3);
            }
            _ => panic!("expected CycleDetected"),
        }
    }

    #[test]
    fn test_topological_sort_valid() {
        let steps = vec![
            Step {
                id: "a".into(),
                cmd: "echo a".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "b".into(),
                cmd: "echo b".into(),
                needs: vec!["a".into()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "c".into(),
                cmd: "echo c".into(),
                needs: vec!["a".into()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "d".into(),
                cmd: "echo d".into(),
                needs: vec!["b".into(), "c".into()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
        ];

        let order = topological_sort(&steps).unwrap();
        assert_eq!(order.len(), 4);
        assert_eq!(order[0], "a");
        // b and c can be in either order
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));
        assert_eq!(order[3], "d");
    }

    #[test]
    fn test_unknown_dependency() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.yaml");
        fs::write(
            &path,
            r#"
name: Bad Workflow
steps:
  - id: step1
    cmd: echo hello
    needs: [nonexistent]
"#,
        )
        .unwrap();

        let result = parse_workflow(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_string_shorthand() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("simple.yaml");
        fs::write(
            &path,
            r#"
name: Simple Pipeline
steps:
  - echo hello
  - echo world
  - echo done
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 3);
        assert_eq!(wf.steps[0].id, "step-1");
        assert_eq!(wf.steps[0].cmd, "echo hello");
        assert!(wf.steps[0].needs.is_empty());
        assert_eq!(wf.steps[1].id, "step-2");
        assert_eq!(wf.steps[1].needs, vec!["step-1"]);
        assert_eq!(wf.steps[2].id, "step-3");
        assert_eq!(wf.steps[2].needs, vec!["step-2"]);
    }

    #[test]
    fn test_parse_cmd_map_without_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cmdmap.yaml");
        fs::write(
            &path,
            r#"
name: Cmd Map Pipeline
steps:
  - cmd: echo first
  - cmd: echo second
    parallel: true
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[0].id, "step-1");
        assert!(wf.steps[0].needs.is_empty());
        assert_eq!(wf.steps[1].id, "step-2");
        assert_eq!(wf.steps[1].needs, vec!["step-1"]);
        assert!(wf.steps[1].parallel);
    }

    #[test]
    fn test_parse_mixed_formats() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mixed.yaml");
        fs::write(
            &path,
            r#"
name: Mixed Pipeline
steps:
  - echo setup
  - cmd: echo build
  - id: deploy
    cmd: echo deploy
    needs: [step-2]
  - echo cleanup
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 4);
        // step-1: string, no deps
        assert_eq!(wf.steps[0].id, "step-1");
        assert!(wf.steps[0].needs.is_empty());
        // step-2: cmd map without id, chains to step-1
        assert_eq!(wf.steps[1].id, "step-2");
        assert_eq!(wf.steps[1].needs, vec!["step-1"]);
        // deploy: explicit id, explicit deps
        assert_eq!(wf.steps[2].id, "deploy");
        assert_eq!(wf.steps[2].needs, vec!["step-2"]);
        // step-4: string, chains to the last auto-id step (step-2)
        assert_eq!(wf.steps[3].id, "step-4");
        assert_eq!(wf.steps[3].needs, vec!["step-2"]);
    }

    #[test]
    fn test_backwards_compatible_full_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("full.yaml");
        fs::write(
            &path,
            r#"
name: Full Format
steps:
  - id: build
    cmd: make build
  - id: test
    cmd: make test
    needs: [build]
  - id: deploy
    cmd: make deploy
    needs: [test]
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps[0].id, "build");
        assert!(wf.steps[0].needs.is_empty());
        assert_eq!(wf.steps[1].id, "test");
        assert_eq!(wf.steps[1].needs, vec!["build"]);
        assert_eq!(wf.steps[2].id, "deploy");
        assert_eq!(wf.steps[2].needs, vec!["test"]);
    }

    #[test]
    fn test_dynamic_env_deferred() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dynenv.yaml");
        fs::write(
            &path,
            r#"
name: Dynamic Env
env:
  STATIC_VAR: hello
  DYNAMIC_VAR:
    cmd: "echo world"
steps:
  - echo $STATIC_VAR $DYNAMIC_VAR
"#,
        )
        .unwrap();

        // Parsing should NOT execute dynamic env commands
        let wf = parse_workflow(&path).unwrap();
        match wf.env.get("STATIC_VAR").unwrap() {
            EnvValue::Static(s) => assert_eq!(s, "hello"),
            _ => panic!("expected static env"),
        }
        match wf.env.get("DYNAMIC_VAR").unwrap() {
            EnvValue::Dynamic { cmd } => assert_eq!(cmd, "echo world"),
            _ => panic!("expected dynamic env"),
        }

        // resolve_env() should execute the commands
        let resolved = resolve_env(&wf.env).unwrap();
        assert_eq!(resolved.get("STATIC_VAR").unwrap(), "hello");
        assert_eq!(resolved.get("DYNAMIC_VAR").unwrap(), "world");
    }

    #[test]
    fn test_dynamic_env_failure_at_resolve() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dynfail.yaml");
        fs::write(
            &path,
            r#"
name: Failing Env
env:
  BAD:
    cmd: "exit 1"
steps:
  - echo should not run
"#,
        )
        .unwrap();

        // Parsing should succeed (env not resolved yet)
        let wf = parse_workflow(&path).unwrap();
        // Resolution should fail
        let result = resolve_env(&wf.env);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("BAD"), "error was: {err}");
    }

    #[test]
    fn test_duplicate_id_detection() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dup.yaml");
        fs::write(
            &path,
            r#"
name: Duplicate IDs
steps:
  - echo first
  - id: step-1
    cmd: echo conflict
"#,
        )
        .unwrap();

        let result = parse_workflow(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("duplicate step id"), "error was: {err}");
    }

    #[test]
    fn test_parse_timeout_field() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("timeout.yaml");
        fs::write(
            &path,
            r#"
name: Timeout Workflow
steps:
  - id: fast
    cmd: echo hello
    timeout: 30
  - id: slow
    cmd: rsync -av /data /backup
    timeout: 300
    needs: [fast]
  - id: notify
    cmd: curl https://example.com
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 3);
        assert_eq!(wf.steps[0].timeout, Some(30));
        assert_eq!(wf.steps[1].timeout, Some(300));
        assert_eq!(wf.steps[2].timeout, None);
    }

    #[test]
    fn test_parse_run_if_field() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("runif.yaml");
        fs::write(
            &path,
            r#"
name: Conditional
steps:
  - id: check
    cmd: echo checking
    run_if: "test -f /tmp/flag"
  - id: always
    cmd: echo always
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps[0].run_if, Some("test -f /tmp/flag".to_string()));
        assert_eq!(wf.steps[1].run_if, None);
    }

    #[test]
    fn test_parse_retry_fields() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("retry.yaml");
        fs::write(
            &path,
            r#"
name: Retry Workflow
steps:
  - id: fetch
    cmd: curl https://example.com
    retry: 3
    retry_delay: 5
  - id: noretry
    cmd: echo done
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps[0].retry, Some(3));
        assert_eq!(wf.steps[0].retry_delay, Some(5));
        assert_eq!(wf.steps[1].retry, None);
        assert_eq!(wf.steps[1].retry_delay, None);
    }

    #[test]
    fn test_parse_secrets_and_notify() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secrets.yaml");
        fs::write(
            &path,
            r#"
name: Secret Workflow
secrets: [DB_PASS, API_KEY]
notify:
  on_failure: "echo failed"
  on_success: "echo ok"
steps:
  - id: s1
    cmd: echo hello
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.secrets, vec!["DB_PASS", "API_KEY"]);
        assert_eq!(wf.notify.on_failure, vec!["echo failed".to_string()]);
        assert_eq!(wf.notify.on_success, vec!["echo ok".to_string()]);
    }

    #[test]
    fn test_parse_without_new_fields_backward_compat() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.yaml");
        fs::write(
            &path,
            r#"
name: Old Style
steps:
  - id: s1
    cmd: echo hello
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert!(wf.secrets.is_empty());
        assert!(wf.notify.on_failure.is_empty());
        assert!(wf.notify.on_success.is_empty());
        assert_eq!(wf.steps[0].run_if, None);
        assert_eq!(wf.steps[0].retry, None);
    }

    #[test]
    fn test_parse_call_field() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("call.yaml");
        fs::write(
            &path,
            r#"
name: Call Workflow
steps:
  - id: preflight
    call: checks/preflight
    outputs:
      - name: status
        pattern: "RESULT:(\\S+)"
  - id: deploy
    cmd: echo deploying
    needs: [preflight]
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[0].call, Some("checks/preflight".to_string()));
        assert!(wf.steps[0].cmd.is_empty());
        assert_eq!(wf.steps[1].call, None);
        assert_eq!(wf.steps[1].cmd, "echo deploying");
    }

    #[test]
    fn test_call_cmd_mutual_exclusion() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad_call.yaml");
        fs::write(
            &path,
            r#"
name: Bad Call
steps:
  - id: both
    cmd: echo hello
    call: other/task
"#,
        )
        .unwrap();

        let result = parse_workflow(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("mutually exclusive"), "error was: {err}");
    }

    #[test]
    fn test_compute_execution_levels() {
        let steps = vec![
            Step {
                id: "a".into(), cmd: "echo a".into(), needs: vec![],
                parallel: false, timeout: None, run_if: None, skip_if: None, retry: None,
                retry_delay: None, interactive: None, outputs: Vec::new(),
                call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "b".into(), cmd: "echo b".into(), needs: vec!["a".into()],
                parallel: false, timeout: None, run_if: None, skip_if: None, retry: None,
                retry_delay: None, interactive: None, outputs: Vec::new(),
                call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "c".into(), cmd: "echo c".into(), needs: vec!["a".into()],
                parallel: false, timeout: None, run_if: None, skip_if: None, retry: None,
                retry_delay: None, interactive: None, outputs: Vec::new(),
                call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
            Step {
                id: "d".into(), cmd: "echo d".into(), needs: vec!["b".into(), "c".into()],
                parallel: false, timeout: None, run_if: None, skip_if: None, retry: None,
                retry_delay: None, interactive: None, outputs: Vec::new(),
                call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
        ];

        let levels = compute_execution_levels(&steps).unwrap();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec!["a"]);
        assert!(levels[1].contains(&"b".to_string()));
        assert!(levels[1].contains(&"c".to_string()));
        assert_eq!(levels[1].len(), 2);
        assert_eq!(levels[2], vec!["d"]);
    }

    #[test]
    fn test_parse_skip_if() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("skipif.yaml");
        fs::write(
            &path,
            r#"
name: Skip If Test
steps:
  - id: deploy
    cmd: echo deploying
  - id: smoke
    cmd: echo testing
    skip_if: "test '1' = '1'"
    needs: [deploy]
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps[0].skip_if, None);
        assert_eq!(wf.steps[1].skip_if, Some("test '1' = '1'".to_string()));
    }

    #[test]
    fn test_parse_notify_with_env() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("notify_env.yaml");
        fs::write(
            &path,
            r#"
name: Notify Env Test
notify:
  on_failure: "echo failed"
  env:
    environment: production
    team: platform
steps:
  - id: s1
    cmd: echo hello
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.notify.on_failure, vec!["echo failed".to_string()]);
        assert_eq!(wf.notify.env.get("environment").unwrap(), "production");
        assert_eq!(wf.notify.env.get("team").unwrap(), "platform");
    }

    #[test]
    fn test_parse_notify_multi_target_array() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("multi_notify.yaml");
        fs::write(
            &path,
            r#"
name: Multi Notify
notify:
  on_failure:
    - "slack://hooks.slack.com/xxx"
    - "ntfy://ntfy.sh/alerts"
  on_success:
    - "webhook://example.com/hook"
steps:
  - id: s1
    cmd: echo hello
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.notify.on_failure, vec![
            "slack://hooks.slack.com/xxx".to_string(),
            "ntfy://ntfy.sh/alerts".to_string(),
        ]);
        assert_eq!(wf.notify.on_success, vec!["webhook://example.com/hook".to_string()]);
    }

    #[test]
    fn test_parse_notify_single_string_compat() {
        // Single string should be deserialized as a one-element Vec
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("single_notify.yaml");
        fs::write(
            &path,
            r#"
name: Single Notify
notify:
  on_failure: "slack://hooks.slack.com/xxx"
steps:
  - id: s1
    cmd: echo hello
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.notify.on_failure.len(), 1);
        assert_eq!(wf.notify.on_failure[0], "slack://hooks.slack.com/xxx");
        assert!(wf.notify.on_success.is_empty());
    }

    #[test]
    #[cfg(feature = "mcp")]
    fn test_parse_mcp_step_alias_server() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp_alias.yaml");
        fs::write(
            &path,
            r#"
name: MCP Alias Test
steps:
  - id: create-issue
    mcp:
      server: github
      tool: create_issue
      args:
        repo: myorg/myapp
        title: Bug report
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 1);
        let mcp = wf.steps[0].mcp.as_ref().expect("mcp field should be set");
        assert!(matches!(&mcp.server, crate::core::models::McpServerRef::Alias(s) if s == "github"));
        assert_eq!(mcp.tool, "create_issue");
        let args = mcp.args.as_ref().unwrap();
        assert_eq!(args["repo"], "myorg/myapp");
        assert_eq!(args["title"], "Bug report");
        // cmd should be empty for mcp-only steps
        assert!(wf.steps[0].cmd.is_empty());
    }

    #[test]
    #[cfg(feature = "mcp")]
    fn test_parse_mcp_step_inline_server() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp_inline.yaml");
        fs::write(
            &path,
            r#"
name: MCP Inline Test
steps:
  - id: list-repos
    mcp:
      server:
        command: "npx @modelcontextprotocol/server-github"
        env:
          GITHUB_TOKEN: "xxx"
        secrets:
          - GITHUB_TOKEN
      tool: list_repos
"#,
        )
        .unwrap();

        let wf = parse_workflow(&path).unwrap();
        assert_eq!(wf.steps.len(), 1);
        let mcp = wf.steps[0].mcp.as_ref().expect("mcp field should be set");
        match &mcp.server {
            crate::core::models::McpServerRef::Inline { command, env, secrets } => {
                assert_eq!(command, "npx @modelcontextprotocol/server-github");
                assert_eq!(env.as_ref().unwrap()["GITHUB_TOKEN"], "xxx");
                assert_eq!(secrets.as_ref().unwrap(), &vec!["GITHUB_TOKEN".to_string()]);
            }
            _ => panic!("expected Inline variant"),
        }
        assert_eq!(mcp.tool, "list_repos");
    }

    #[test]
    fn test_parse_mcp_and_cmd_mutual_exclusion() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp_cmd.yaml");
        fs::write(
            &path,
            r#"
name: Bad MCP
steps:
  - id: both
    cmd: echo hello
    mcp:
      server: github
      tool: create_issue
"#,
        )
        .unwrap();

        let result = parse_workflow(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("mutually exclusive"), "error was: {err}");
        assert!(err.contains("cmd"), "error was: {err}");
        assert!(err.contains("mcp"), "error was: {err}");
    }

    #[test]
    #[cfg(feature = "mcp")]
    fn test_parse_mcp_step_empty_tool_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp_no_tool.yaml");
        fs::write(
            &path,
            r#"
name: Bad MCP No Tool
steps:
  - id: no-tool
    mcp:
      server: github
      tool: ""
"#,
        )
        .unwrap();

        let result = parse_workflow(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no 'tool'"), "error was: {err}");
    }

    #[test]
    #[cfg(not(feature = "mcp"))]
    fn test_parse_mcp_step_rejected_without_feature() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp_no_feature.yaml");
        fs::write(
            &path,
            r#"
name: MCP Without Feature
steps:
  - id: mcp-step
    mcp:
      server: github
      tool: create_issue
"#,
        )
        .unwrap();

        let result = parse_workflow(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("requires the mcp feature"), "error was: {err}");
        assert!(err.contains("cargo build --features mcp"), "error was: {err}");
    }
}
