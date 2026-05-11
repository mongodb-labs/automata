# automata — Tech Spec

> Status: draft — 2026-05-11  
> Author: filipe.menezes@mongodb.com

---

## Problem

Automation logic (Jira lifecycle, issue sync, Dependabot auto-merge) is duplicated across 16 repos with ~120 workflow files. When the pattern changes — a new Jira field, a different transition ID, a new repo to onboard — every repo must be updated individually. There is no central place to observe, test, or iterate on these automations.

---

## Goal

A declarative, Kanopy-hosted CI platform (`automata`) that:

1. Receives GitHub App webhook events from any repo in the org
2. Matches events against a set of **automation YAML files** (one per automation, inspired by GitHub Actions workflow files and Evergreen CI's function system)
3. Executes each matching automation as an observable, retriable Argo Workflow
4. Lets anyone add or modify an automation by editing a YAML file and opening a PR — no Rust required for common cases

---

## Non-goals

- Replacing CI/CD pipelines (lint, test, build, release) — those stay as GitHub Actions in each repo
- Real-time latency requirements — webhook-to-action in seconds is fine
- Hosting a UI — Argo Workflows UI and Splunk cover observability

---

## Architecture

```
GitHub Repos (any repo in the org)
  │
  │  webhook (HMAC-signed, github well-known source)
  ▼
Argo Events EventSource  ← self-service, no ENTSEC approval
  │
EventBus (NATS JetStream)
  │
Sensor (one, routes all events)
  │
  ▼  event payload as parameter
Engine WorkflowTemplate  ←  automata engine --event '<json>'
  │
  │  reads automations/*.yaml, matches trigger, fans out
  ▼
Per-automation WorkflowTemplates (generated at build time from YAML files)
  │
  │  one container step per fn: call
  ▼
automata fn <name> --inputs '<json>'  ← Rust function library
  ├── GitHub API  (ApixBot installation token)
  └── Jira API
```

### Why Argo Events + Argo Workflows

- GitHub is a **well-known source** in Kanopy — self-service, no ENTSEC ticket
- Argo Events handles webhook signature validation and NATS fan-out
- Argo Workflows gives per-step observability (`workflows.staging.corp.mongodb.com`), Prometheus metrics, and native retry/parallelism
- Both available in all Kanopy clusters (beta — acceptable for internal automation)

---

## Platform Concepts

### Automation files (`automations/*.yaml`)

Inspired by **GitHub Actions** workflow files. One YAML file = one automation. Each file declares:
- `on:` — when to run (GHA-style event trigger with filters)
- `repos:` — which repos this automation applies to
- `jobs:` — one or more parallel jobs (v1: single job, parallelism deferred)
- `steps:` — sequential steps within a job, each calling a built-in `fn:` or a named reusable `uses:`

### Functions (`functions/*.yaml`)

Inspired by **Evergreen CI** named functions. Reusable step sequences that can be called from any automation with `uses:`. Avoids copy-pasting steps across automations.

### Repo config (`config/repos.yaml`)

Per-repo values (Jira project, component, team field) available as `${{ config.* }}` expressions inside automation steps. Kept separate from automation logic so a new repo can be onboarded without touching the automations.

### Expression syntax

`${{ }}` — same as GitHub Actions. Evaluated at runtime by the engine. Available contexts:

| Context | Content |
|---|---|
| `${{ event.* }}` | Raw GitHub webhook payload |
| `${{ config.* }}` | Resolved repo config from `config/repos.yaml` |
| `${{ steps.<id>.outputs.* }}` | Output from a previous step |
| `${{ inputs.* }}` | Inputs passed to a named function via `uses:` |
| `${{ env.* }}` | Environment variables |

---

## Automation YAML Format

```yaml
# automations/jira-lifecycle.yaml
name: jira-lifecycle
description: Open a Jira ticket when a PR is opened; resolve it when merged.

on:
  pull_request:
    types: [opened]
    filters:
      actor:
        exclude: [dependabot[bot]]

repos:
  - mongodb/mongodb-atlas-cli
  - mongodb/mongodb-atlas-local
  - mongodb/atlas-github-action
  - mongodb-labs/cobra2snooty
  - mongodb/openapi

jobs:
  open-ticket:
    steps:
      - fn: jira.create_story
        id: ticket
        with:
          project: "${{ config.jira.project }}"
          component: "${{ config.jira.component }}"
          team_field: "${{ config.jira.team_custom_field }}"
          team_value: "${{ config.jira.team_value }}"
          summary: "[${{ event.repository.name }}] ${{ event.pull_request.title }}"

      - fn: github.post_comment
        with:
          body: "Jira ticket: ${{ steps.ticket.outputs.url }}"

      - fn: github.add_label
        with:
          label: auto_close_jira

---
# automations/jira-lifecycle-close.yaml
name: jira-lifecycle-close
description: Resolve the Jira ticket when a labeled PR is merged.

on:
  pull_request:
    types: [closed]
    filters:
      merged: true
      labels:
        include: [auto_close_jira]

repos:
  - mongodb/mongodb-atlas-cli
  - mongodb/mongodb-atlas-local
  - mongodb/atlas-github-action
  - mongodb-labs/cobra2snooty
  - mongodb/openapi

jobs:
  close-ticket:
    steps:
      - fn: jira.find_key
        id: find
        with:
          branch: "${{ event.pull_request.head.ref }}"
          comments_url: "${{ event.pull_request.comments_url }}"
          pattern: "${{ config.jira.project }}-\\d+"

      - fn: jira.transition
        with:
          key: "${{ steps.find.outputs.key }}"
          transition_id: "1381"

---
# automations/issue-sync.yaml
name: issue-sync
description: Sync GitHub issue lifecycle to Jira.

on:
  issues:
    types: [opened, closed, reopened]

repos:
  - mongodb/mongodb-atlas-cli
  - mongodb-js/mongodb-mcp-server
  - mongodb/atlas-github-action

jobs:
  sync:
    steps:
      - fn: jira.create_story
        id: ticket
        if: "${{ event.action == 'opened' }}"
        with:
          project: "${{ config.jira.project }}"
          component: "${{ config.jira.component }}"
          summary: "[${{ event.repository.name }}] ${{ event.issue.title }}"

      - fn: github.post_comment
        if: "${{ event.action == 'opened' }}"
        with:
          body: "Jira ticket: ${{ steps.ticket.outputs.url }}"

      - fn: jira.find_key
        id: find
        if: "${{ event.action != 'opened' }}"
        with:
          comments_url: "${{ event.issue.comments_url }}"
          pattern: "${{ config.jira.project }}-\\d+"

      - fn: jira.transition
        if: "${{ event.action == 'closed' }}"
        with:
          key: "${{ steps.find.outputs.key }}"
          transition_id: "1381"

      - fn: jira.transition
        if: "${{ event.action == 'reopened' }}"
        with:
          key: "${{ steps.find.outputs.key }}"
          transition_id: "1351"

---
# automations/dependabot-merge.yaml
name: dependabot-merge
description: Auto-approve and merge Dependabot PRs.

on:
  pull_request:
    types: [opened]
    filters:
      actor:
        include: [dependabot[bot]]

repos:
  - mongodb/mongodb-atlas-cli
  - mongodb/mongodb-atlas-local
  - mongodb/apix-action
  - 10gen/apix-bot
  - mongodb/atlas-local-lib
  - mongodb-js/atlas-local-lib-js

jobs:
  auto-merge:
    steps:
      - fn: github.approve_pr

      - fn: github.enable_auto_merge
        with:
          strategy: squash
```

---

## Named Functions Format

Inspired by **Evergreen CI** named functions — reusable step sequences called via `uses:` from any automation.

```yaml
# functions/notify-slack.yaml
name: notify-slack
description: Post a message to a Slack channel.
inputs:
  - name: channel
    required: true
  - name: message
    required: true

steps:
  - fn: slack.post_message
    with:
      channel: "${{ inputs.channel }}"
      text: "${{ inputs.message }}"
```

Called from an automation:

```yaml
steps:
  - fn: jira.create_story
    id: ticket
    with: ...

  - uses: notify-slack
    with:
      channel: "${{ env.SLACK_CHANNEL_ID }}"
      message: "New ticket ${{ steps.ticket.outputs.key }}"
```

---

## Built-in Function Library (Rust)

These are the primitive operations implemented in Rust. New functions are added to the library and become available to all automations.

| Function | Inputs | Outputs |
|---|---|---|
| `jira.create_story` | `project`, `component`, `summary`, `team_field`, `team_value` | `key`, `url` |
| `jira.transition` | `key`, `transition_id` | — |
| `jira.find_key` | `comments_url` or `branch`, `pattern` | `key` |
| `github.post_comment` | `body` | `comment_id` |
| `github.add_label` | `label` | — |
| `github.approve_pr` | — | `review_id` |
| `github.enable_auto_merge` | `strategy` | — |
| `slack.post_message` | `channel`, `text` | `ts` |

Functions are invoked by the engine as container steps:

```
automata fn jira.create_story \
  --inputs '{"project":"CLOUDP","component":"AtlasCLI",...}' \
  --event  '{"repository":{"full_name":"mongodb/mongodb-atlas-cli"},...}' \
  --config '{"jira":{"project":"CLOUDP",...}}'
```

Output JSON is written to stdout and captured by Argo as a step output parameter.

---

## Repo Layout

```
automata/
├── .drone.yml
├── Dockerfile                         # multi-stage: cargo build → distroless/static
├── Cargo.toml
├── src/
│   ├── main.rs                        # clap: `automata fn` and `automata generate`
│   ├── engine.rs                      # loads automations/*.yaml, matches triggers
│   ├── config.rs                      # loads config/repos.yaml, resolves per-repo config
│   ├── expr.rs                        # ${{ }} expression evaluator
│   ├── github.rs                      # GitHub App auth + API client
│   ├── jira.rs                        # Jira REST client
│   └── functions/
│       ├── mod.rs                     # function registry + dispatch
│       ├── jira.rs                    # jira.* built-ins
│       ├── github.rs                  # github.* built-ins
│       └── slack.rs                   # slack.* built-ins
├── automations/                       # declarative automation files
│   ├── jira-lifecycle.yaml
│   ├── jira-lifecycle-close.yaml
│   ├── issue-sync.yaml
│   └── dependabot-merge.yaml
├── functions/                         # reusable named step sequences
│   └── notify-slack.yaml
├── config/
│   └── repos.yaml                     # per-repo Jira config, baked into image
├── k8s/
│   ├── eventsource.yaml               # GitHub EventSource (hand-written)
│   └── generated/                     # output of `automata generate`, committed
│       ├── sensor.yaml
│       └── workflow-templates.yaml
└── deploy/
    └── eventbus-values.yaml           # mongodb/argo-eventbus chart values
```

---

## Build-time Generation

During the Drone build, `automata generate` reads all `automations/*.yaml` files and emits the k8s manifests needed to run them:

1. **One Sensor** with dependency filters covering all event types across all automations
2. **One WorkflowTemplate per automation** — each step in the template calls `automata fn <name> --inputs ...`

The generated manifests are committed to `k8s/generated/` so they can be reviewed in PRs like any other code change.

```yaml
# .drone.yml (abbreviated)
steps:
  - name: generate
    image: 795250896452.dkr.ecr.us-east-1.amazonaws.com/skunkworks/automata:git-${DRONE_COMMIT_SHA:0:7}
    commands:
      - automata generate --automations automations/ --functions functions/ --output k8s/generated/
    when:
      event: [push]

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

  - name: deploy-eventbus
    image: public.ecr.aws/kanopy/drone-helm:v3
    settings:
      chart: mongodb/argo-eventbus
      chart_version: TBD
      add_repos: [mongodb=https://10gen.github.io/helm-charts]
      namespace: skunkworks
      release: automata-eventbus
      values_files: ["deploy/eventbus-values.yaml"]
      api_server: https://api.staging.corp.mongodb.com
      kubernetes_token:
        from_secret: staging_kubernetes_token

  - name: apply-k8s
    image: bitnami/kubectl
    environment:
      KUBE_TOKEN:
        from_secret: staging_kubernetes_token
    commands:
      - kubectl apply -f k8s/eventsource.yaml
      - kubectl apply -f k8s/generated/
```

---

## Repo Config (`config/repos.yaml`)

Per-repo values available as `${{ config.* }}` in automations. Uses defaults + per-repo overrides.

```yaml
defaults:
  jira:
    project: CLOUDP
    team_custom_field: customfield_12751
    team_value: "<JIRA_TEAM_APIX_2>"

repos:
  mongodb/mongodb-atlas-cli:
    jira:
      component: AtlasCLI

  mongodb/mongodb-atlas-local:
    jira:
      component: AtlasLocal

  mongodb/atlas-local-lib:
    jira:
      component: AtlasLocal

  mongodb/atlas-local-cli:
    jira:
      component: AtlasLocal

  mongodb-js/atlas-local-lib-js:
    jira:
      component: AtlasLocal

  mongodb-js/mongodb-mcp-server:
    jira:
      project: TBD
      component: MCP
      team_value: TBD

  mongodb/atlas-github-action:
    jira:
      component: AtlasCLI

  mongodb/openapi:
    jira:
      component: OpenAPI
      team_value: "<JIRA_TEAM_ID_APIX_PLATFORM>"

  10gen/apix-bot:
    jira:
      component: ApixBot

  mongodb/apix-action:
    jira:
      component: ApixAction

  mongodb-labs/cobra2snooty:
    jira:
      component: AtlasCLI

  mongodb-forks/chocolatey-packages: {}
  mongodb-forks/digest: {}
  mongodb/atlas-cli-core: {}
```

---

## Argo Events Configuration

### EventSource (`k8s/eventsource.yaml`)

Single EventSource listening to all configured repos. GitHub well-known source — self-service.

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
          names: [mongodb-atlas-cli, mongodb-atlas-local, apix-action, atlas-github-action,
                  atlas-local-lib, atlas-local-cli, openapi]
        - owner: mongodb-js
          names: [mongodb-mcp-server, atlas-local-lib-js]
        - owner: 10gen
          names: [apix-bot]
        - owner: mongodb-labs
          names: [cobra2snooty]
        - owner: mongodb-forks
          names: [chocolatey-packages, digest]
      webhook:
        endpoint: /github
        port: "12000"
        method: POST
      webhookSecret:
        name: automata-secrets
        key: GITHUB_WEBHOOK_SECRET
      events: ["*"]
      insecure: false
      active: true
      contentType: json
```

Webhook URL:
```
https://webhooks.staging.corp.mongodb.com/skunkworks/automata-github/github
```

### Sensor (generated)

`automata generate` emits a single Sensor that routes all events to the engine WorkflowTemplate, passing the full payload as a parameter. The engine then does trigger matching in Rust.

```yaml
# k8s/generated/sensor.yaml (example of generated output)
apiVersion: argoproj.io/v1alpha1
kind: Sensor
metadata:
  name: automata-sensor
  namespace: skunkworks
spec:
  dependencies:
    - name: github-dep
      eventSourceName: automata-github
      eventName: automata
  triggers:
    - template:
        name: run-engine
        argoWorkflow:
          operation: submit
          source:
            resource:
              apiVersion: argoproj.io/v1alpha1
              kind: WorkflowTemplate
              name: automata-engine
          parameters:
            - src:
                dependencyName: github-dep
                dataKey: body
              dest: spec.arguments.parameters.0.value
            - src:
                dependencyName: github-dep
                dataKey: body.repository.full_name
              dest: spec.arguments.parameters.1.value
      rateLimit:
        unit: Second
        requestsPerUnit: 1
```

---

## Secrets Setup

```bash
helm ksec set automata-secrets \
  GITHUB_APP_ID=<apixbot-app-id> \
  GITHUB_APP_PRIVATE_KEY=<pem> \
  GITHUB_WEBHOOK_SECRET=<min-12-chars> \
  JIRA_BASE_URL=https://jira.mongodb.org \
  JIRA_USER=<email> \
  JIRA_API_TOKEN=<token> \
  SLACK_BEARER_TOKEN=<token>
```

Drone secrets:
```bash
drone secret add <repo> --name=ecr_access_key        --data=<value>
drone secret add <repo> --name=ecr_secret_key        --data=<value>
drone secret add <repo> --name=staging_kubernetes_token --data=<value>
```

---

## Observability

| Signal | Where |
|---|---|
| Workflow success/failure per automation | Argo UI (`workflows.staging.corp.mongodb.com`) |
| Step logs | Argo UI live → Splunk after GC (`index=skunkworks-staging`) |
| Run count / duration / status | Prometheus → Grafana (`argo_workflows_skunkworks_*`) |
| Webhook delivery | GitHub App webhook deliveries page |

---

## Onboarding a New Repo

1. Add the repo to `k8s/eventsource.yaml` repositories list
2. Add a `config/repos.yaml` entry (or rely on defaults)
3. Add the repo to the `repos:` list in whichever `automations/*.yaml` apply
4. Register the webhook in the repo settings: `https://webhooks.staging.corp.mongodb.com/skunkworks/automata-github/github`
5. Open a PR — Drone rebuilds and redeploys everything

---

## Adding a New Automation

1. Create `automations/my-automation.yaml` with `on:`, `repos:`, and `steps:`
2. If it needs a new built-in function, add it to `src/functions/` in Rust and register it in `src/functions/mod.rs`
3. Open a PR — `automata generate` runs in CI, the generated manifests are committed, Drone deploys

---

## Open Questions

- **Staging only**: `skunkworks` namespace is staging-only. Production deployment needs a separate namespace and prod cluster credentials.
- **Jira project per repo**: `mongodb-mcp-server` and `openapi` use different projects/team values — confirm exact keys before filling `config/repos.yaml` TBDs
- **ApixBot installation scope**: confirm ApixBot is installed on all 16 target repos
- **Rate limit exception**: if Dependabot opens many PRs simultaneously, the default 1/s Sensor rate limit may need a KANOPY ticket exception
- **`automata generate` bootstrap**: the first build needs the binary to exist before it can generate manifests — solve with a two-step pipeline or commit initial generated output manually
