use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::core::models::{RawStep, RawWorkflow, Step, Workflow};
use crate::error::{DzError, Result};

/// Parse a YAML workflow file into a Workflow struct.
/// Supports three step formats: bare strings, maps without id, and full maps.
pub fn parse_workflow(path: &Path) -> Result<Workflow> {
    let contents = std::fs::read_to_string(path)?;
    let raw: RawWorkflow = serde_yaml::from_str(&contents)?;
    let steps = normalize_steps(raw.steps)?;
    let workflow = Workflow {
        name: raw.name,
        steps,
        env: raw.env,
        workdir: raw.workdir,
    };

    validate_workflow(&workflow)?;
    Ok(workflow)
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
                Step { id, cmd, needs, parallel: false }
            }
            RawStep::CmdMap { id: None, cmd, needs: _, parallel } => {
                let id = format!("step-{}", i + 1);
                let needs = prev_auto_id.take().into_iter().collect();
                prev_auto_id = Some(id.clone());
                Step { id, cmd, needs, parallel }
            }
            RawStep::CmdMap { id: Some(id), cmd, needs, parallel } => {
                // Explicit id: no implicit chaining, but don't break the chain for others
                Step { id, cmd, needs, parallel }
            }
        };

        if !used_ids.insert(step.id.clone()) {
            return Err(DzError::Parse(format!("duplicate step id '{}'", step.id)));
        }
        steps.push(step);
    }

    Ok(steps)
}

/// Wrap a shell script as a single-step workflow.
pub fn parse_shell_task(path: &Path) -> Result<Workflow> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("script")
        .to_string();

    let cmd = format!("bash {}", path.display());

    Ok(Workflow {
        name,
        workdir: None,
        steps: vec![Step {
            id: "run".to_string(),
            cmd,
            needs: Vec::new(),
            parallel: false,
        }],
        env: HashMap::new(),
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
        assert_eq!(wf.env.get("FOO").unwrap(), "bar");
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
            },
            Step {
                id: "b".into(),
                cmd: "echo b".into(),
                needs: vec!["a".into()],
                parallel: false,
            },
            Step {
                id: "c".into(),
                cmd: "echo c".into(),
                needs: vec!["b".into()],
                parallel: false,
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
            },
            Step {
                id: "b".into(),
                cmd: "echo b".into(),
                needs: vec!["a".into()],
                parallel: false,
            },
            Step {
                id: "c".into(),
                cmd: "echo c".into(),
                needs: vec!["a".into()],
                parallel: false,
            },
            Step {
                id: "d".into(),
                cmd: "echo d".into(),
                needs: vec!["b".into(), "c".into()],
                parallel: false,
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
}
