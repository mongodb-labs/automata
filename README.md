# automata

Declarative CI automation hub for the MongoDB APIx DevTools org. Receives GitHub App webhooks via [Argo Events](https://argoproj.github.io/argo-events/) and executes automation rules defined in YAML — no Rust required for common cases.

## How it works

```
GitHub repo  →  Argo Events EventSource  →  NATS EventBus  →  Sensor (http trigger)  →  automata
```

1. Argo Events validates the GitHub HMAC signature and fans out events onto NATS JetStream.
2. The Sensor POSTs the payload to `POST /webhook/github` with an `X-Automata-Token` header.
3. automata matches the event against every automation file in the configured directory and runs the matching ones sequentially.

## Automation files

One YAML file = one automation. Example:

```yaml
name: jira-lifecycle-atlascli
pipeline:
  - given:
      trigger: github
      repos:
        - mongodb/mongodb-atlas-cli
    when:
      - event: pull_request
        action: opened
        actor_not: dependabot[bot]
    then:
      - jira.create_issue:
          id: ticket
          issue_type: Story
          project: CLOUDP
          component: AtlasCLI
          summary: "[{payload.repository.name}] {payload.pull_request.title}"
      - github.post_comment:
          body: "Jira ticket: {ticket.url}"
      - github.add_label:
          label: auto_close_jira
```

`pipeline:` is a list of trigger blocks. `when:` items within a block are OR'd; keys within an item are AND'd. `then:` steps run sequentially; each step can reference outputs from previous steps via `{step-id.field}`.

## Built-in functions

| Function | Key inputs | Outputs |
|---|---|---|
| `jira.create_issue` | `project`, `issue_type`, `component`, `summary`, `custom_fields` | `key`, `url` |
| `jira.transition` | `key`, `transition_id` | — |
| `github.post_comment` | `owner`, `repo`, `number`, `body` | `comment_id` |
| `github.add_label` | `owner`, `repo`, `number`, `label` | — |
| `github.approve_pr` | `owner`, `repo`, `number` | `review_id` |
| `github.enable_auto_merge` | `owner`, `repo`, `number`, `strategy` | — |
| `github.list_pr_comments` | `owner`, `repo`, `number` | `comments` (array) |
| `builtin.jq` | `input` (step id), `expr` (jq expression) | fields of the object if `expr` returns one, otherwise `result` |

`owner`, `repo`, and `number` are typically interpolated from the event payload:

```yaml
owner: "{payload.repository.owner.login}"
repo: "{payload.repository.name}"
number: "{payload.pull_request.number}"
```

## Adding an automation

1. Create `automations/my-automation.yaml` with `given:`, `when:`, and `then:`.
2. Add the repo to `deploy/eventsource.yaml` under the appropriate owner if it isn't listed there yet.
3. Open a PR — Drone builds and deploys automatically on merge to `main`.

If the automation needs a new built-in function, add it to `src/functions/` in Rust and register it in `src/functions/mod.rs`.

## Onboarding a new repo

1. Add the repo to `deploy/eventsource.yaml` under the appropriate owner.
2. Add it to the `given.repos:` list in whichever `automations/*.yaml` files apply.
3. Open a PR — the EventSource will register the GitHub webhook automatically on deploy.

## Running locally

```bash
export GITHUB_APP_ID=<id>
export GITHUB_APP_PRIVATE_KEY="$(cat /path/to/private-key.pem)"
export GITHUB_WEBHOOK_SECRET=<secret>
export SENSOR_TOKEN=<token>
export JIRA_BASE_URL=https://jira.mongodb.org
export JIRA_API_TOKEN=<token>

cargo run -- automations/
```

The first argument is the path to the automations directory (defaults to `.`).

Simulate an event (Sensor envelope format):

```bash
curl -X POST http://localhost:8080/webhook/github \
  -H "Content-Type: application/json" \
  -H "X-Automata-Token: $SENSOR_TOKEN" \
  -d '{
    "github_event": "pull_request",
    "body": {
      "action": "opened",
      "repository": {"full_name": "mongodb-labs/automata", "name": "automata"},
      "pull_request": {"number": 1, "title": "Test PR", "head": {"ref": "fix/test"}},
      "sender": {"login": "alice"}
    }
  }'
```

## Endpoints

| Endpoint | Description |
|---|---|
| `POST /webhook/github` | Receives Sensor-wrapped GitHub events |
| `GET /doctor` | HTML table: GitHub App installation and webhook status per repo |
| `GET /health` | Liveness check |

`/doctor` is also available at `/` (redirects).

## Deployment

Drone builds and deploys on every push to `main`:

1. `test` — `cargo test`
2. `build-and-push` — builds image, pushes to ECR (`skunkworks/automata`)
3. `deploy-service` — Helm `mongodb/web-app` chart to `skunkworks` namespace
4. `deploy-eventbus` — Helm `mongodb/argo-eventbus` chart
5. `apply-k8s` — `kubectl apply` for `deploy/eventsource.yaml` and `deploy/sensor.yaml`

Staging URL: `https://automata.skunkworks.staging.corp.mongodb.com`

Secrets are managed with `helm ksec` under the `automata-secrets` Kubernetes Secret:

```bash
helm ksec set automata-secrets \
  GITHUB_APP_ID=<id> \
  GITHUB_APP_PRIVATE_KEY="$(cat key.pem)" \
  GITHUB_WEBHOOK_SECRET=<secret> \
  SENSOR_TOKEN=<token> \
  JIRA_BASE_URL=https://jira.mongodb.org \
  JIRA_API_TOKEN=<token>
```
