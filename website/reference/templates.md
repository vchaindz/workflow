---
outline: [2, 3]
---

# Template Catalog

Workflow ships with a library of ready-to-use templates covering common sysadmin, container, Kubernetes, and infrastructure tasks. Use them as starting points or install them directly.

**8 categories, 55 templates.**

Browse templates in the TUI (`t` key) or list them from the CLI:

```bash
workflow templates
workflow templates --fetch   # download community templates
```

---

## Docker (10)

### Docker Cleanup

Remove stale containers, dangling images, unused networks, and build cache (only resources older than 24h)

| | |
|---|---|
| **File** | `templates/docker/cleanup.yaml` |
| **Steps** | 6 |

### Docker Compose Status

Check status of a docker compose project and its services

| | |
|---|---|
| **File** | `templates/docker/compose-status.yaml` |
| **Steps** | 4 |
| **Variables** | `project` |

### Docker Container Status

Overview of all containers, resource usage, and health checks

| | |
|---|---|
| **File** | `templates/docker/container-status.yaml` |
| **Steps** | 4 |

### Docker Image Update Check

Check running containers for newer base images available upstream

| | |
|---|---|
| **File** | `templates/docker/image-update.yaml` |
| **Steps** | 3 |

### Docker Container Logs

List running containers and tail recent logs from a selected one

| | |
|---|---|
| **File** | `templates/docker/logs-tail.yaml` |
| **Steps** | 4 |
| **Variables** | `container`, `lines` |

### Docker Network Inspection

List networks, show container connectivity, and check for issues

| | |
|---|---|
| **File** | `templates/docker/network-inspect.yaml` |
| **Steps** | 4 |
| **Variables** | `network` |

### Docker Resource Limits Check

Audit container resource limits - memory, CPU, and restart policies

| | |
|---|---|
| **File** | `templates/docker/resource-limits.yaml` |
| **Steps** | 4 |

### Restart Unhealthy Containers

Find and restart containers with unhealthy or exited status

| | |
|---|---|
| **File** | `templates/docker/restart-unhealthy.yaml` |
| **Steps** | 4 |

### Docker Security Overview

Check container security posture - privileged mode, capabilities, root users

| | |
|---|---|
| **File** | `templates/docker/security-scan.yaml` |
| **Steps** | 4 |
| **Variables** | `container` |

### Docker Volume Backup

List volumes, show usage, and create a tar backup of a named volume

| | |
|---|---|
| **File** | `templates/docker/volume-backup.yaml` |
| **Steps** | 3 |
| **Variables** | `volume_name`, `backup_path` |

## Kubectl (10)

### Cluster Health Check

Overview of Kubernetes cluster health - nodes, components, and capacity

| | |
|---|---|
| **File** | `templates/kubectl/cluster-health.yaml` |
| **Steps** | 5 |
| **Variables** | `context` |

### Deployment Status

Check deployment health, rollout status, and replica counts

| | |
|---|---|
| **File** | `templates/kubectl/deployment-status.yaml` |
| **Steps** | 5 |
| **Variables** | `namespace` |

### Failed Pod Diagnostics

Find CrashLoopBackOff and Error pods, show logs and events

| | |
|---|---|
| **File** | `templates/kubectl/failed-pods.yaml` |
| **Steps** | 4 |
| **Variables** | `namespace` |

### Namespace Audit

Audit namespaces - resource counts, quotas, and stale namespaces

| | |
|---|---|
| **File** | `templates/kubectl/namespace-audit.yaml` |
| **Steps** | 4 |

### Pod Status Overview

List pods across namespaces, find unhealthy pods, and show restart counts

| | |
|---|---|
| **File** | `templates/kubectl/pod-status.yaml` |
| **Steps** | 5 |
| **Variables** | `namespace` |

### Persistent Volume & Storage Check

Audit persistent volumes, claims, and storage classes

| | |
|---|---|
| **File** | `templates/kubectl/pv-storage.yaml` |
| **Steps** | 5 |

### RBAC Review

Audit cluster roles, bindings, and service accounts for security review

| | |
|---|---|
| **File** | `templates/kubectl/rbac-review.yaml` |
| **Steps** | 5 |

### Resource Usage Report

Show CPU and memory usage across nodes, pods, and namespaces

| | |
|---|---|
| **File** | `templates/kubectl/resource-usage.yaml` |
| **Steps** | 6 |

### Secret & ConfigMap Audit

List secrets and configmaps, find unused ones, check for expiring TLS certs

| | |
|---|---|
| **File** | `templates/kubectl/secret-configmap-audit.yaml` |
| **Steps** | 4 |
| **Variables** | `namespace` |

### Service & Endpoint Check

List services, check endpoints, and verify ingress configuration

| | |
|---|---|
| **File** | `templates/kubectl/service-endpoints.yaml` |
| **Steps** | 4 |
| **Variables** | `namespace` |

## Mcp (3)

### MCP Database Backup

Query a database for table sizes, export results to a file, and notify on completion

| | |
|---|---|
| **File** | `templates/mcp/db-backup.yaml` |
| **Steps** | 4 |
| **Variables** | `database_url`, `backup_dir`, `notify_channel` |

### MCP Filesystem Operations

Read, transform, and write files using the filesystem MCP server

| | |
|---|---|
| **File** | `templates/mcp/filesystem-ops.yaml` |
| **Steps** | 5 |
| **Variables** | `source_dir`, `dest_dir`, `file_pattern` |

### MCP GitHub Release

Create a GitHub release, post to Slack, and close the milestone issue using MCP servers

| | |
|---|---|
| **File** | `templates/mcp/github-release.yaml` |
| **Steps** | 3 |
| **Variables** | `repo`, `tag`, `release_title`, `slack_channel`, `issue_number` |

## Monitoring (1)

### Website Content Check

Check a website for content changes using curl and diff

| | |
|---|---|
| **File** | `templates/monitoring/website-content-check.yaml` |
| **Steps** | 1 |
| **Variables** | `url`, `check_string` |

## Patching (10)

### Changelog Review

Show changelogs for pending updates before applying (apt changelog, dnf changelog, zypper)

| | |
|---|---|
| **File** | `templates/patching/changelog-review.yaml` |
| **Steps** | 2 |
| **Variables** | `max_entries` |

### Held Packages Manager

List, hold, and unhold packages — pin versions across distributions

| | |
|---|---|
| **File** | `templates/patching/held-packages.yaml` |
| **Steps** | 1 |
| **Variables** | `action`, `package` |

### Kernel Update

Kernel-specific update with old kernel cleanup and optional reboot scheduling

| | |
|---|---|
| **File** | `templates/patching/kernel-update.yaml` |
| **Steps** | 5 |
| **Variables** | `keep_kernels`, `reboot_delay` |

### Patch Audit

List available patches without applying — shows pending security and bugfix counts per distribution

| | |
|---|---|
| **File** | `templates/patching/patch-audit.yaml` |
| **Steps** | 3 |

### Patch Compliance Report

Generate a patch compliance report with installed date, pending count, CVE exposure, and kernel age

| | |
|---|---|
| **File** | `templates/patching/patch-report.yaml` |
| **Steps** | 6 |

### Post-Patch Verification

Post-patch verification — confirm package integrity, check for broken deps, validate running kernel matches installed

| | |
|---|---|
| **File** | `templates/patching/patch-verify.yaml` |
| **Steps** | 5 |

### Reboot Check

Check if reboot is needed post-patch and list services needing restart (needrestart, needs-restarting, zypper ps)

| | |
|---|---|
| **File** | `templates/patching/reboot-check.yaml` |
| **Steps** | 3 |

### Patch Rollback

Rollback last patch operation using distro-specific mechanisms (dnf history, apt-mark, zypper rollback, pacman cache)

| | |
|---|---|
| **File** | `templates/patching/rollback.yaml` |
| **Steps** | 3 |

### Security Patches Only

Apply security-only patches across Linux distributions (Debian/Ubuntu, RHEL/Fedora, SUSE, Arch)

| | |
|---|---|
| **File** | `templates/patching/security-patches.yaml` |
| **Steps** | 3 |

### Unattended Updates Setup

Configure automatic security updates (unattended-upgrades for Debian/Ubuntu, dnf-automatic for RHEL/Fedora, zypper for SUSE)

| | |
|---|---|
| **File** | `templates/patching/unattended-setup.yaml` |
| **Steps** | 4 |

## Security (1)

### Trivy CVE Check

Scan running Docker containers for CVEs using Trivy

| | |
|---|---|
| **File** | `templates/security/trivy-cve-check.yaml` |
| **Steps** | 1 |
| **Variables** | `severity` |

## Sysadmin (16)

### Backup Directory Verification

Verify backup directory exists, check recent files, age of newest backup, and total size

| | |
|---|---|
| **File** | `templates/sysadmin/backup-verify.yaml` |
| **Steps** | 4 |
| **Variables** | `backup_dir` |

### CPU & Load Analysis

Show system load, top CPU consumers, core count, and recent high-load syslog entries

| | |
|---|---|
| **File** | `templates/sysadmin/cpu-load.yaml` |
| **Steps** | 5 |

### Cron Job Audit

List all user crontabs and system cron directories for a complete audit

| | |
|---|---|
| **File** | `templates/sysadmin/cron-audit.yaml` |
| **Steps** | 4 |

### Disk Usage Report

Show filesystem usage, flag partitions over threshold, and find largest directories

| | |
|---|---|
| **File** | `templates/sysadmin/disk-usage.yaml` |
| **Steps** | 4 |
| **Variables** | `threshold`, `scan_path` |

### Failed Systemd Services

Find failed systemd units, show details, and suggest recovery actions

| | |
|---|---|
| **File** | `templates/sysadmin/failed-services.yaml` |
| **Steps** | 3 |

### Firewall Review

Dump firewall rules from ufw, firewalld, or iptables

| | |
|---|---|
| **File** | `templates/sysadmin/firewall-review.yaml` |
| **Steps** | 5 |

### Journal & Log Cleanup

Clean up systemd journal and rotated log files to free disk space

| | |
|---|---|
| **File** | `templates/sysadmin/log-cleanup.yaml` |
| **Steps** | 5 |
| **Variables** | `retention_days` |

### Memory & Swap Report

Check memory usage, top consumers, and swap status

| | |
|---|---|
| **File** | `templates/sysadmin/memory-check.yaml` |
| **Steps** | 4 |

### NTP Sync Check

Check time synchronization status via chronyd, timedatectl, or ntpd

| | |
|---|---|
| **File** | `templates/sysadmin/ntp-sync-check.yaml` |
| **Steps** | 3 |

### Listening Ports Check

Show listening ports, connection counts, and firewall status

| | |
|---|---|
| **File** | `templates/sysadmin/port-scan.yaml` |
| **Steps** | 4 |

### Systemd Service Health Check

Check status, logs, and enabled state of a systemd service

| | |
|---|---|
| **File** | `templates/sysadmin/service-status.yaml` |
| **Steps** | 3 |
| **Variables** | `service_name` |

### SMART Disk Health

Check SMART attributes and health status for storage devices

| | |
|---|---|
| **File** | `templates/sysadmin/smart-disk-health.yaml` |
| **Steps** | 4 |
| **Variables** | `device` |

### SSH Key Audit

Audit SSH keys for weak algorithms, incorrect permissions, and age

| | |
|---|---|
| **File** | `templates/sysadmin/ssh-key-audit.yaml` |
| **Steps** | 4 |
| **Variables** | `scan_path` |

### SSL Certificate Expiry Check

Check SSL certificate expiry dates for domains and warn if expiring soon

| | |
|---|---|
| **File** | `templates/sysadmin/ssl-cert-expiry.yaml` |
| **Steps** | 1 |
| **Variables** | `domains`, `warn_days` |

### System Package Update

Detect package manager, update package lists, upgrade packages, and check reboot status

| | |
|---|---|
| **File** | `templates/sysadmin/system-update.yaml` |
| **Steps** | 3 |

### User Account Audit

Audit user accounts, check for empty passwords, show recent logins, and list sudo users

| | |
|---|---|
| **File** | `templates/sysadmin/user-audit.yaml` |
| **Steps** | 5 |

## Tools (4)

### Claude CLI Update

Update Claude Code CLI to the latest version

| | |
|---|---|
| **File** | `templates/tools/claude-update.yaml` |
| **Steps** | 3 |

### Codex CLI Update

Update OpenAI Codex CLI to the latest version

| | |
|---|---|
| **File** | `templates/tools/codex-update.yaml` |
| **Steps** | 3 |

### Git Sync Pull

Pull latest workflow changes from remote repository

| | |
|---|---|
| **File** | `templates/tools/git-sync-pull.yaml` |
| **Steps** | 2 |
| **Variables** | `branch` |

### Git Sync Push

Commit and push workflow changes to remote repository

| | |
|---|---|
| **File** | `templates/tools/git-sync.yaml` |
| **Steps** | 3 |
| **Variables** | `branch`, `message` |

---

*This page is auto-generated by `website/scripts/generate-template-catalog.sh`.*
