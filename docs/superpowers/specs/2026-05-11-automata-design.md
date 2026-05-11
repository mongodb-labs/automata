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
3. Executes each matching automation as sequential API calls with structured logging
4. Lets anyone add or modify an automation by editing a YAML file and opening a PR — no Rust required for common cases

---

## Non-goals

- Replacing CI/CD pipelines (lint, test, build, release) — those stay as GitHub Actions in each repo
- Real-time latency requirements — webhook-to-action in seconds is fine
- Hosting a UI — Splunk covers log observability

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
Sensor — http trigger  ← no k8s rate limit
  │
  │  POST full payload
  ▼
automata  (axum HTTP server, mongodb/web-app)
  ├── POST /webhook/github    — GitHub App events (HMAC-SHA256 validated)
  ├── POST /webhook/slack     — future: Slack events
  ├── POST /webhook/jira      — future: Jira webhooks
  ├── POST /webhook/evergreen — future: Evergreen CI events
  └── GET  /doctor            — ApixBot installation status across all configured repos
  │
  │  loads automations/*.yaml, matches when: conditions, executes then: steps
  ▼
Built-in function library (Rust)
  ├── GitHub API  (ApixBot installation token)
  └── Jira API
```

### Why Argo Events + automata HTTP server

- **Argo Events** and **Argo Workflows** are independent — Argo Events can trigger any HTTP endpoint, not just Argo Workflows
- GitHub is a **well-known source** in Kanopy — self-service, no ENTSEC ticket
- Argo Events handles webhook HMAC validation and NATS fan-out
- Sensor `http` trigger POSTs directly to the automata service — no container-per-step overhead
- The 1/s Kanopy rate limit applies to **k8s triggers only** — http triggers are not subject to it
- automata deployed as a `mongodb/web-app` service — single pod, structured logs to Splunk, Prometheus metrics via `/metrics`

---

## Platform Concepts

### Automation files (`automations/*.yaml`)

One YAML file = one automation. Each file declares:
- `given:` — static context: trigger source (`github`, `slack`, `jira`, `evergreen`) and repo list
- `when:` — list of condition groups; items are OR'd, keys within an item are AND'd
- `then:` — sequential steps, each calling a built-in function or a named reusable `uses:`

Literal values (Jira project, component, team field) are hardcoded directly in the file. When a group of repos needs different values, a separate automation file is created for that group.

### Functions (`functions/*.yaml`)

Inspired by **Evergreen CI** named functions. Reusable step sequences that can be called from any automation with `uses:`. Accepts typed `inputs:` passed at call site.

### Expression syntax

Any string value may contain `{path}` expressions. At runtime the engine calls `interpolate(value, ctx)`, which finds all `{...}` spans via regex, resolves each dotted path against the execution context, and substitutes the result.

Available context paths:

| Path | Content |
|---|---|
| `payload.*` | Raw GitHub webhook payload |
| `<step-id>.<output>` | Output from a previous step, e.g. `{ticket.url}` |
| `inputs.*` | Inputs passed to a named function via `uses:` |

---

## Automation YAML Format

Different repo groups get separate files when they need different literal values (e.g. different Jira component or project). There is no shared config system — values are hardcoded directly in each automation file.

```yaml
# automations/jira-lifecycle-atlascli.yaml
name: jira-lifecycle-atlascli
description: Open a Jira ticket when a PR is opened.
given:
  trigger: github
  repos:
    - mongodb/mongodb-atlas-cli
    - mongodb/atlas-github-action
    - mongodb-labs/cobra2snooty
when:
  - event: pull_request
    action: opened
    actor_not: dependabot[bot]
then:
  - jira.create_story:
      id: ticket
      project: CLOUDP
      component: AtlasCLI
      custom_fields:
        customfield_12751: "<JIRA_TEAM_APIX_2>"
      summary: "[{payload.repository.name}] {payload.pull_request.title}"
  - github.post_comment:
      body: "Jira ticket: {ticket.url}"
  - github.add_label:
      label: auto_close_jira

---
# automations/jira-lifecycle-close.yaml
name: jira-lifecycle-close
description: Resolve the Jira ticket when a labeled PR is merged.
given:
  trigger: github
  repos:
    - mongodb/mongodb-atlas-cli
    - mongodb/mongodb-atlas-local
    - mongodb/atlas-github-action
    - mongodb-labs/cobra2snooty
    - mongodb/openapi
when:
  - event: pull_request
    action: closed
    merged: true
    labels_include: [auto_close_jira]
then:
  - jira.find_key:
      id: find
      pattern: "CLOUDP-\\d+"
      branch: "{payload.pull_request.head.ref}"
      comments_url: "{payload.pull_request.comments_url}"
  - jira.transition:
      key: "{find.key}"
      transition_id: "1381"

---
# automations/issue-sync-atlascli.yaml
name: issue-sync-atlascli
description: Sync GitHub issue lifecycle to Jira for AtlasCLI repos.
given:
  trigger: github
  repos:
    - mongodb/mongodb-atlas-cli
    - mongodb/atlas-github-action
when:
  - event: issues
    action: [opened, closed, reopened]
then:
  - jira.create_story:
      id: ticket
      if: action_is_opened
      project: CLOUDP
      component: AtlasCLI
      summary: "[{payload.repository.name}] {payload.issue.title}"
  - github.post_comment:
      if: action_is_opened
      body: "Jira ticket: {ticket.url}"
  - jira.find_key:
      id: find
      if: action_not_opened
      pattern: "CLOUDP-\\d+"
      comments_url: "{payload.issue.comments_url}"
  - jira.transition:
      if: action_is_closed
      key: "{find.key}"
      transition_id: "1381"
  - jira.transition:
      if: action_is_reopened
      key: "{find.key}"
      transition_id: "1351"

---
# automations/dependabot-merge.yaml
name: dependabot-merge
description: Auto-approve and merge Dependabot PRs.
given:
  trigger: github
  repos:
    - mongodb/mongodb-atlas-cli
    - mongodb/mongodb-atlas-local
    - mongodb/apix-action
    - 10gen/apix-bot
    - mongodb/atlas-local-lib
    - mongodb-js/atlas-local-lib-js
when:
  - event: pull_request
    action: opened
    actor: dependabot[bot]
then:
  - github.approve_pr: {}
  - github.enable_auto_merge:
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
  - slack.post_message:
      channel: "{inputs.channel}"
      text: "{inputs.message}"
```

Called from an automation:

```yaml
then:
  - jira.create_story:
      id: ticket
      project: CLOUDP
      # ...
  - uses: notify-slack
      channel: C12345678
      message: "New ticket: {ticket.key}"
```

---

## Built-in Function Library (Rust)

These are the primitive operations implemented in Rust. New functions are added to the library and become available to all automations.

| Function | Inputs | Outputs |
|---|---|---|
| `jira.create_story` | `project`, `component`, `summary`, `custom_fields` (map) | `key`, `url` |
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
  --inputs  '{"project":"CLOUDP","component":"AtlasCLI",...}' \
  --payload '{"repository":{"full_name":"mongodb/mongodb-atlas-cli"},...}'
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
│   ├── expr.rs                        # !ref resolver and {key} string interpolator
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
├── k8s/
│   ├── eventsource.yaml               # GitHub EventSource (hand-written)
│   └── sensor.yaml                    # Sensor with http trigger (hand-written)
└── deploy/
    └── eventbus-values.yaml           # mongodb/argo-eventbus chart values
```

---

## Drone Pipeline

Build, push image, deploy web service and event infrastructure on every push to `main`.

```yaml
# .drone.yml (abbreviated)
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

  - name: deploy-service
    image: public.ecr.aws/kanopy/drone-helm:v3
    settings:
      chart: mongodb/web-app
      chart_version: TBD
      add_repos: [mongodb=https://10gen.github.io/helm-charts]
      namespace: skunkworks
      release: automata
      values_files: ["deploy/staging.yaml"]
      values: image.tag=git-${DRONE_COMMIT_SHA:0:7}
      api_server: https://api.staging.corp.mongodb.com
      kubernetes_token:
        from_secret: staging_kubernetes_token

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
      - kubectl apply -f k8s/sensor.yaml
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

### Sensor (`k8s/sensor.yaml`)

Single Sensor with an `http` trigger — POSTs the full GitHub payload to the automata web service. Trigger matching happens inside automata in Rust, not in the Sensor. The 1/s Kanopy rate limit applies to k8s triggers only; http triggers are not subject to it.

```yaml
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
        name: call-automata
        http:
          url: http://automata.skunkworks.svc.cluster.local/webhook/github
          method: POST
          headers:
            - name: Content-Type
              value: application/json
          payload:
            - src:
                dependencyName: github-dep
                dataKey: body
              dest: body
          secureHeaders:
            - name: X-Automata-Token
              valueFrom:
                secretKeyRef:
                  name: automata-secrets
                  key: SENSOR_TOKEN
          timeoutSeconds: 30
```

---

## Secrets Setup

```bash
helm ksec set automata-secrets \
  GITHUB_APP_ID=<apixbot-app-id> \
  GITHUB_APP_PRIVATE_KEY=<pem> \
  GITHUB_WEBHOOK_SECRET=<min-12-chars> \
  SENSOR_TOKEN=<min-12-chars> \
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
| Automation run logs | Splunk (`index=skunkworks-staging`) |
| Run count / duration / error rate | Prometheus → Grafana (custom metrics from automata `/metrics`) |
| Webhook delivery | GitHub App webhook deliveries page |
| Sensor trigger history | Argo Events UI (`workflows.staging.corp.mongodb.com` → Event Flow) |

---

## Onboarding a New Repo

1. Add the repo to `k8s/eventsource.yaml` repositories list
2. Add the repo to the `given.repos:` list in whichever `automations/*.yaml` apply (create a new automation file if it needs different literal values)
3. Register the webhook in the repo settings: `https://webhooks.staging.corp.mongodb.com/skunkworks/automata-github/github`
4. Open a PR — Drone rebuilds and redeploys everything

---

## Adding a New Automation

1. Create `automations/my-automation.yaml` with `given:`, `when:`, and `then:`
2. If it needs a new built-in function, add it to `src/functions/` in Rust and register it in `src/functions/mod.rs`
3. Open a PR — Drone builds and deploys the updated image; no manifest generation needed

---

## Open Questions

- **Staging only**: `skunkworks` namespace is staging-only. Production deployment needs a separate namespace and prod cluster credentials.
- **`GET /doctor`**: endpoint that reads all `given.repos` entries across `automations/*.yaml` and checks each repo via the GitHub API for ApixBot installation status — returns a JSON report of installed / missing repos. Replaces the manual confirmation step.
- **Sensor http trigger availability**: Kanopy doesn't explicitly document the `http` trigger type — verify it works in `skunkworks` before committing to the architecture
