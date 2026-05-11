# automata — Tech Spec

> Status: draft — 2026-05-11  
> Author: filipe.menezes@mongodb.com

---

## Problem

Automation logic (Jira lifecycle, issue sync, Dependabot auto-merge) is duplicated across 16 repos with ~120 workflow files. When the pattern changes — a new Jira field, a different transition ID, a new repo to onboard — every repo must be updated individually. There is no central place to observe, test, or iterate on these automations.

---

## Goal

A single Kanopy-hosted automation hub (`automata`) that:

1. Receives GitHub App webhook events from any repo in the org
2. Routes each event to the appropriate automation logic
3. Executes that logic as an observable, retriable Argo Workflow
4. Replaces the duplicated GitHub Actions patterns across the estate

---

## Non-goals

- Replacing CI/CD pipelines (lint, test, build, release) — those stay as GitHub Actions in each repo
- Real-time latency requirements — webhook-to-action in seconds is fine; sub-second is not needed
- Hosting a UI — Argo Workflows UI and Splunk are sufficient for observability

---

## Architecture

```
GitHub Repos (any repo in the org)
  │
  │  webhook (HMAC-signed, github well-known source)
  ▼
Argo Events EventSource  ← self-service, no ENTSEC approval needed
  │
EventBus (NATS JetStream)  ← mongodb/argo-eventbus helm chart
  │
Sensors (one per event type)  ← raw k8s manifests, kubectl-applied by Drone
  │  filter + route
  ▼
Argo WorkflowTemplates  ← mongodb/argo-workflow-catalog helm chart
  │
  │  container step(s)
  ▼
automata binary (Rust)
  ├── GitHub API  via ApixBot installation token
  └── Jira API    via Jira REST API token
```

### Why Argo Events + Argo Workflows

- GitHub is a **well-known source** in Kanopy: self-service webhook setup, no approval process
- Argo Events handles signature validation and fan-out over NATS
- Argo Workflows gives per-step observability in the UI (`workflows.staging.corp.mongodb.com`), built-in Prometheus metrics, and native retry/parallelism
- Both are available in all Kanopy clusters (beta status — not suitable for hard SLA requirements, acceptable for internal automation)

---

## Repo Layout

```
automata/
├── .drone.yml                        # build image → deploy all helm/k8s resources
├── Dockerfile                        # multi-stage: cargo build → distroless/static
├── Cargo.toml                        # workspace root
├── src/
│   ├── main.rs                       # clap CLI, subcommands dispatch to handlers
│   ├── github.rs                     # GitHub App auth (installation token) + API client
│   ├── jira.rs                       # Jira REST client
│   └── handlers/
│       ├── jira_lifecycle.rs         # PR opened/merged ↔ Jira create/resolve
│       ├── issue_sync.rs             # issue opened/closed/reopened ↔ Jira
│       └── dependabot.rs             # Dependabot PR → approve + auto-merge
├── k8s/
│   ├── eventsource.yaml              # GitHub EventSource (raw manifest)
│   ├── sensor-jira-lifecycle.yaml
│   ├── sensor-issue-sync.yaml
│   └── sensor-dependabot.yaml
└── deploy/
    ├── eventbus-values.yaml          # mongodb/argo-eventbus chart values
    └── workflows-values.yaml         # mongodb/argo-workflow-catalog chart values
```

---

## Rust Binary

The binary is a CLI built with `clap`. Each subcommand maps to one automation. Argo Workflow steps call it as a container command, passing the raw GitHub webhook payload via `--payload`.

```
automata <subcommand> --payload '<json>'
```

| Subcommand | Trigger |
|---|---|
| `jira-lifecycle-open` | PR opened, actor ≠ dependabot |
| `jira-lifecycle-close` | PR merged, labeled `auto_close_jira` |
| `issue-sync-open` | Issue opened |
| `issue-sync-close` | Issue closed |
| `issue-sync-reopen` | Issue reopened |
| `dependabot-merge` | PR opened, actor = `dependabot[bot]` |

All configuration is injected via environment variables from the `automata-secrets` k8s secret:

| Variable | Purpose |
|---|---|
| `GITHUB_APP_ID` | ApixBot App ID |
| `GITHUB_APP_PRIVATE_KEY` | ApixBot PEM private key |
| `GITHUB_WEBHOOK_SECRET` | HMAC secret for EventSource signature validation |
| `JIRA_BASE_URL` | e.g. `https://jira.mongodb.org` |
| `JIRA_USER` | Service account email |
| `JIRA_API_TOKEN` | Jira REST API token |

---

## Handlers

### `jira-lifecycle-open` — PR opened → create Jira

**Input**: `pull_request` event, `action: opened`, actor ≠ `dependabot[bot]`

Steps:
1. Mint GitHub App installation token for the repo's installation
2. Create CLOUDP Jira Story with:
   - Summary: `[<repo>] <PR title>`
   - `customfield_12751`: `vars.JIRA_TEAM_APIX_2`
   - Component: derived from repo (e.g. `AtlasCLI`)
   - `fixVersions`: current version from Jira (or omit if not applicable)
3. Post PR comment: `Jira ticket: <URL>`
4. Add label `auto_close_jira` to the PR

**Output**: Jira ticket URL logged to stdout (captured in Argo Workflow archive)

---

### `jira-lifecycle-close` — PR merged → resolve Jira

**Input**: `pull_request` event, `action: closed`, `merged: true`, label `auto_close_jira` present

Steps:
1. Find Jira ticket key:
   - If branch name matches `CLOUDP-\d+` → use that
   - Otherwise, fetch PR comments and grep for `CLOUDP-\d+` pattern
2. Transition ticket to Resolved/Fixed (transition ID `1381`)

---

### `issue-sync-open` — Issue opened → create Jira

**Input**: `issues` event, `action: opened`

Steps:
1. Create Jira Story (same field mapping as jira-lifecycle-open)
2. Post issue comment: `Jira ticket: <URL>`

---

### `issue-sync-close` — Issue closed → close Jira

**Input**: `issues` event, `action: closed`

Steps:
1. Find Jira ticket key from issue comments (grep `CLOUDP-\d+`)
2. Transition ticket — use `1381` (Resolved/Fixed) for normal closes; `1371` (Won't Fix) reserved for explicit won't-fix flows not yet in scope

---

### `issue-sync-reopen` — Issue reopened → reopen Jira

**Input**: `issues` event, `action: reopened`

Steps:
1. Find Jira ticket key from issue comments
2. Transition ticket (reopen, transition ID `1351`)

---

### `dependabot-merge` — Dependabot PR → auto-merge

**Input**: `pull_request` event, `action: opened`, actor = `dependabot[bot]`

Steps:
1. Mint GitHub App installation token
2. Approve the PR (submit review with `APPROVE`)
3. Enable auto-merge (squash strategy) via GitHub API

---

## Argo Events Configuration

### EventBus

Managed by `mongodb/argo-eventbus` chart (NATS JetStream). One per namespace.

```yaml
# deploy/eventbus-values.yaml
name: automata-bus
```

### EventSource

Raw k8s manifest (`k8s/eventsource.yaml`). GitHub well-known source — self-service, no ENTSEC ticket.

```yaml
apiVersion: argoproj.io/v1alpha1
kind: EventSource
metadata:
  name: automata-github
  namespace: skunkworks
  annotations:
    v1alpha1.argoslower.kanopy-platform/known-source: "github"
spec:
  eventBusName: automata-bus
  github:
    automata:
      repositories:
        - owner: mongodb
          names: ["mongodb-atlas-cli", "mongodb-atlas-local", ...]
        - owner: mongodb-js
          names: ["mongodb-mcp-server"]
        - owner: 10gen
          names: ["apix-bot"]
      webhook:
        endpoint: /github
        port: "12000"
        method: POST
      webhookSecret:
        name: automata-secrets
        key: GITHUB_WEBHOOK_SECRET
      events:
        - pull_request
        - issues
      insecure: false
      active: true
      contentType: json
```

Webhook URL (staging):
```
https://webhooks.staging.corp.mongodb.com/skunkworks/automata-github/github
```

### Sensors

One Sensor per automation. Each filters the EventSource payload and submits the matching WorkflowTemplate.

Routing between `jira-lifecycle-open` and `dependabot-merge` is done at the Sensor layer via `body.sender.login` — not in the binary — so only one WorkflowTemplate fires per event:
- `sensor-jira-lifecycle-open`: `action=opened` AND `sender.login != dependabot[bot]`
- `sensor-dependabot`: `action=opened` AND `sender.login = dependabot[bot]`

Example — `k8s/sensor-jira-lifecycle.yaml`:

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Sensor
metadata:
  name: sensor-jira-lifecycle-open
spec:
  dependencies:
    - name: github-dep
      eventSourceName: automata-github
      eventName: automata
      filters:
        data:
          - path: body.action
            type: string
            value: ["opened"]
          - path: body.sender.login
            type: string
            comparator: "!="
            value: ["dependabot[bot]"]
  triggers:
    - template:
        name: trigger-jira-lifecycle-open
        argoWorkflow:
          operation: submit
          source:
            resource:
              apiVersion: argoproj.io/v1alpha1
              kind: WorkflowTemplate
              name: jira-lifecycle-open
          parameters:
            - src:
                dependencyName: github-dep
                dataKey: body
              dest: spec.arguments.parameters.0.value
      rateLimit:
        unit: Second
        requestsPerUnit: 1
```

---

## Argo WorkflowTemplates

Managed by `mongodb/argo-workflow-catalog` chart. Defined in `deploy/workflows-values.yaml`.

Each WorkflowTemplate is a single-step workflow that runs the `automata` container:

```yaml
# deploy/workflows-values.yaml
workflowTemplates:
  - name: jira-lifecycle-open
    serviceAccountName: automata-sa
    arguments:
      parameters:
        - name: payload
    templates:
      - name: run
        container:
          image: 795250896452.dkr.ecr.us-east-1.amazonaws.com/skunkworks/automata:latest
          command: ["automata"]
          args: ["jira-lifecycle-open", "--payload", "{{inputs.parameters.payload}}"]
          envFrom:
            - secretRef:
                name: automata-secrets
  # ... one entry per subcommand
```

---

## Drone Pipeline

```yaml
# .drone.yml (abbreviated)
kind: pipeline
type: kubernetes
name: default
platform:
  arch: arm64
trigger:
  branch: [main]

steps:
  - name: build-and-push
    image: plugins/kaniko-ecr
    settings:
      registry: 795250896452.dkr.ecr.us-east-1.amazonaws.com
      repo: skunkworks/automata
      tags: [git-${DRONE_COMMIT_SHA:0:7}, latest]
      create_repository: true
      access_key:
        from_secret: ecr_access_key
      secret_key:
        from_secret: ecr_secret_key
    when:
      event: [push]

  - name: deploy-eventbus
    image: public.ecr.aws/kanopy/drone-helm:v3
    settings:
      chart: mongodb/argo-eventbus
      chart_version: TBD  # look up latest at https://github.com/10gen/helm-charts/tree/master/charts/argo-eventbus
      add_repos: [mongodb=https://10gen.github.io/helm-charts]
      namespace: skunkworks
      release: automata-eventbus
      values_files: ["deploy/eventbus-values.yaml"]
      api_server: https://api.staging.corp.mongodb.com
      kubernetes_token:
        from_secret: staging_kubernetes_token

  - name: deploy-workflows
    image: public.ecr.aws/kanopy/drone-helm:v3
    settings:
      chart: mongodb/argo-workflow-catalog
      chart_version: TBD  # look up latest at https://github.com/10gen/helm-charts/tree/master/charts/argo-workflow-catalog
      add_repos: [mongodb=https://10gen.github.io/helm-charts]
      namespace: skunkworks
      release: automata-workflows
      values_files: ["deploy/workflows-values.yaml"]
      api_server: https://api.staging.corp.mongodb.com
      kubernetes_token:
        from_secret: staging_kubernetes_token

  - name: apply-eventsource-and-sensors
    image: bitnami/kubectl
    environment:
      KUBE_TOKEN:
        from_secret: staging_kubernetes_token
    commands:
      - kubectl apply -f k8s/eventsource.yaml
      - kubectl apply -f k8s/sensor-jira-lifecycle.yaml
      - kubectl apply -f k8s/sensor-issue-sync.yaml
      - kubectl apply -f k8s/sensor-dependabot.yaml
    when:
      event: [push]
```

---

## Secrets Setup

All secrets stored as a single k8s secret via `helm ksec`:

```bash
helm ksec set automata-secrets \
  GITHUB_APP_ID=<apixbot-app-id> \
  GITHUB_APP_PRIVATE_KEY=<pem-contents> \
  GITHUB_WEBHOOK_SECRET=<min-12-chars> \
  JIRA_BASE_URL=https://jira.mongodb.org \
  JIRA_USER=<service-account-email> \
  JIRA_API_TOKEN=<token>
```

Drone secrets (for ECR + k8s deployment):

```bash
drone secret add <repo> --name=ecr_access_key --data=<value>
drone secret add <repo> --name=ecr_secret_key --data=<value>
drone secret add <repo> --name=staging_kubernetes_token --data=<value>
```

---

## Observability

| Signal | Where |
|---|---|
| Workflow success/failure | Argo UI (`workflows.staging.corp.mongodb.com`) |
| Step logs | Argo UI (during run) → Splunk after GC (`index=skunkworks-staging`) |
| Run count / duration / status | Prometheus → Grafana (`argo_workflows_skunkworks_*`) |
| Webhook delivery | GitHub App webhook deliveries page |

---

## Repo Onboarding

To add a new repo to `automata`:

1. Add it to the `repositories` list in `k8s/eventsource.yaml`
2. Register the webhook in the repo settings pointing to `https://webhooks.staging.corp.mongodb.com/skunkworks/automata-github/github`, with the same `GITHUB_WEBHOOK_SECRET`
3. Push to `main` — Drone re-applies the EventSource

No code changes needed unless the new repo requires different Jira fields or components (add a per-repo mapping in `src/jira.rs`).

---

## Jira Field Mapping per Repo

The current estate uses `CLOUDP` tickets everywhere but with slightly different field values. A static mapping in `src/jira.rs`:

```rust
pub struct RepoConfig {
    pub jira_project: &'static str,   // e.g. "CLOUDP"
    pub jira_component: &'static str, // e.g. "AtlasCLI"
    pub jira_team_field: &'static str, // customfield_12751 value
}
```

Looked up by `repo.full_name` at runtime.

---

## Open Questions

- **Staging only**: `skunkworks` namespace is staging-only. If this moves to production in the future, a new namespace + prod cluster deployment will be needed.
- **Jira project per repo**: `atlas-cli` uses `CLOUDP`, `mongodb-mcp-server` uses a different project — confirm the full mapping before implementation
- **ApixBot installation scope**: confirm ApixBot is installed on all 16 target repos
- **Rate limit exception**: if Dependabot opens many PRs simultaneously, the default 1/s Sensor rate limit may need a KANOPY ticket exception
