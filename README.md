# automata

Declarative CI automation hub for the MongoDB APIX org. Receives GitHub App webhooks via [Argo Events](https://argoproj.github.io/argo-events/) and executes automation rules defined in YAML — no Rust required for common cases.

## How it works

```
GitHub repo  →  Argo Events EventSource  →  NATS EventBus  →  Sensor (http trigger)  →  automata
```

1. Argo Events validates the GitHub HMAC signature and fans out events onto NATS JetStream.
2. The Sensor POSTs the payload to `POST /webhook/github` with an `X-Automata-Token` header.
3. automata matches the event against every `automations/*.yaml` file and runs the matching ones sequentially.

## Automation files

One YAML file = one automation. Example:

```yaml
name: jira-lifecycle-atlascli
description: Open a Jira ticket when a PR is opened.
given:
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

`when:` items are OR'd; keys within an item are AND'd. `then:` steps run sequentially; each step can reference outputs from previous steps via `{step-id.field}`.

## Built-in functions

| Function | Key inputs | Outputs |
|---|---|---|
| `jira.create_issue` | `project`, `issue_type`, `component`, `summary`, `custom_fields` | `key`, `url` |
| `jira.transition` | `key`, `transition_id` | — |
| `jira.find_key` | `comments_url` or `branch`, `pattern` | `key` |
| `github.post_comment` | `body` | `comment_id` |
| `github.add_label` | `label` | — |
| `github.approve_pr` | — | `review_id` |
| `github.enable_auto_merge` | `strategy` | — |
| `slack.post_message` | `channel`, `text` | `ts` |

## Named functions

Reusable step sequences live in `functions/*.yaml` and are called via `uses:`:

```yaml
then:
  - uses: notify-slack
    channel: C12345678
    message: "New ticket: {ticket.key}"
```

## Adding an automation

1. Create `automations/my-automation.yaml` with `given:`, `when:`, and `then:`.
2. Add the repo to `deploy/eventsource.yaml` if it isn't listed there yet.
3. Open a PR — Drone builds and deploys the updated image automatically.

If the automation needs a new built-in function, add it to `src/functions/` in Rust and register it in `src/functions/mod.rs`.

## Onboarding a new repo

1. Add the repo to `deploy/eventsource.yaml` under the appropriate owner.
2. Add it to the `given.repos:` list in whichever `automations/*.yaml` files apply.
3. Register the webhook in the repo settings pointing to the Argo Events webhook URL.
4. Open a PR.

## Running locally

```bash
export GITHUB_APP_ID=<id>
export GITHUB_APP_PRIVATE_KEY="$(cat /path/to/private-key.pem)"
export GITHUB_WEBHOOK_SECRET=<secret>
export SENSOR_TOKEN=<token>
export JIRA_BASE_URL=https://jira.mongodb.org
export JIRA_USER=<email>
export JIRA_API_TOKEN=<token>

cargo run
```

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
| `GET /doctor` | Reports ApixBot installation status across all configured repos |
| `GET /health` | Liveness check |

## Deployment

Drone builds and deploys on every push to `main`:

1. `test` — `cargo test`
2. `build-and-push` — builds image, pushes to ECR (`skunkworks/automata`)
3. `deploy-service` — Helm `mongodb/web-app` chart to `skunkworks` namespace
4. `deploy-eventbus` — Helm `mongodb/argo-eventbus` chart
5. `apply-k8s` — `kubectl apply` for `deploy/eventsource.yaml` and `deploy/sensor.yaml`

Secrets are managed with `helm ksec` under the `automata-secrets` Kubernetes Secret.
