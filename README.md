# automata

Declarative CI automation hub for the MongoDB APIx DevTools org. Receives GitHub App webhooks via [Argo Events](https://argoproj.github.io/argo-events/) and executes automation rules defined in YAML — no Rust required for common cases.

## How it works

```
GitHub repo  →  Argo Events EventSource  →  NATS EventBus  →  Sensor (http trigger)  →  automata
```

1. Argo Events validates the GitHub HMAC signature and fans out events onto NATS JetStream.
2. The Sensor POSTs the payload to `POST /webhook/github` with an `X-Automata-Token` header.
3. automata matches the event against every automation file in the configured directory and runs the matching ones sequentially.

---

## Automation file reference

One YAML file = one automation. The top-level structure is:

```yaml
name: <string>          # unique identifier, must match the filename
pipeline:               # list of trigger blocks (see below)
  - given: ...
    when: ...
    then: ...
```

### `given:`

Declares what triggers this pipeline entry.

```yaml
given:
  trigger: github       # only "github" is supported today
  repos:                # list of "owner/repo" strings this entry applies to
    - mongodb/mongodb-atlas-cli
    - mongodb-labs/automata
```

### `when:`

A list of condition groups. The entry fires if **any** group matches (OR). Within a group every key is AND'd.

```yaml
when:
  - event: pull_request         # string or list of strings
    action: opened              # string or list of strings
    actor: alice                # sender login must equal this
    merged: true                # pull_request.merged must equal this
    label: auto_close_jira      # string or list — all listed labels must be present (AND)
    exclude:                    # list of condition groups; entry is skipped if any matches (OR)
      - actor: dependabot[bot]
```

All `when:` keys are optional. Omitting a key means "match anything" for that dimension.

#### `event`

The GitHub event type string sent in the `X-GitHub-Event` header.

```yaml
event: pull_request             # single event
event: [pull_request, issues]   # fires on either
```

Common values: `pull_request`, `issues`, `push`, `issue_comment`, `pull_request_review`.

#### `action`

The `action` field inside the webhook payload.

```yaml
action: opened
action: [opened, reopened]
```

Common values depend on the event: `opened`, `closed`, `reopened`, `labeled`, `synchronize`, `submitted`.

#### `actor`

The `sender.login` field in the payload. Exact string match.

```yaml
actor: dependabot[bot]
```

#### `merged`

Matches `pull_request.merged` (boolean). Useful for distinguishing a merged close from an abandoned close.

```yaml
merged: true
```

#### `label`

All listed labels must be present on the pull request (AND semantics).

```yaml
label: auto_close_jira
label: [auto_close_jira, reviewed]
```

#### `exclude:`

A list of condition groups with the same shape as a `when:` item (minus `exclude:` itself). The entry is skipped if **any** exclude group matches.

```yaml
exclude:
  - actor: dependabot[bot]
  - action: labeled             # also skip labeled events
```

### `then:`

A list of steps executed sequentially. Each step is a mapping with the function name as the only key:

```yaml
then:
  - jira.create_issue:
      id: ticket                # optional: name this step's output for later reference
      if: action_is_opened      # optional: skip this step unless condition is true
      project: CLOUDP
      summary: "[{payload.repository.name}] {payload.pull_request.title}"
```

#### `id:`

Names the step output so later steps can reference it via `{id.field}`.

#### `if:`

Skips the step unless the condition evaluates to true. Supported values:

| Condition | Meaning |
|---|---|
| `action_is_opened` | `payload.action == "opened"` |
| `action_is_closed` | `payload.action == "closed"` |
| `action_is_reopened` | `payload.action == "reopened"` |
| `action_not_opened` | `payload.action != "opened"` |

#### Interpolation

Any string value in a step's inputs can embed `{path}` expressions:

| Expression | Resolves to |
|---|---|
| `{payload.repository.name}` | field from the GitHub webhook payload |
| `{payload.pull_request.number}` | nested payload field |
| `{step-id.field}` | named output from a previous step |

---

## Built-in functions

### `jira.create_issue`

Creates a Jira issue and exposes its key and URL as step outputs.

| Input | Required | Description |
|---|---|---|
| `project` | ✅ | Jira project key, e.g. `CLOUDP` |
| `issue_type` | ✅ | e.g. `Story`, `Bug`, `Task` |
| `summary` | ✅ | Issue title; supports `{payload.*}` interpolation |
| `component` | | Jira component name |
| `custom_fields` | | Map of custom field IDs to values |

Outputs: `key` (e.g. `CLOUDP-1234`), `url` (full Jira URL).

### `jira.transition`

Moves a Jira issue to a new status.

| Input | Required | Description |
|---|---|---|
| `key` | ✅ | Jira issue key |
| `transition_id` | ✅ | Numeric transition ID from the Jira workflow |

### `github.post_comment`

Posts a comment on a PR or issue.

| Input | Required | Description |
|---|---|---|
| `owner` | ✅ | Repository owner |
| `repo` | ✅ | Repository name |
| `number` | ✅ | PR or issue number |
| `body` | ✅ | Comment body; supports interpolation |

Outputs: `comment_id`.

### `github.add_label`

Adds a label to a PR or issue.

| Input | Required | Description |
|---|---|---|
| `owner` | ✅ | |
| `repo` | ✅ | |
| `number` | ✅ | |
| `label` | ✅ | Label name (must already exist on the repo) |

### `github.approve_pr`

Submits an approving review on a PR.

| Input | Required | Description |
|---|---|---|
| `owner` | ✅ | |
| `repo` | ✅ | |
| `number` | ✅ | |

Outputs: `review_id`.

### `github.enable_auto_merge`

Enables auto-merge on a PR.

| Input | Required | Description |
|---|---|---|
| `owner` | ✅ | |
| `repo` | ✅ | |
| `number` | ✅ | |
| `strategy` | | Merge strategy: `merge`, `squash` (default), `rebase` |

### `github.list_pr_comments`

Fetches all comments on a PR or issue.

| Input | Required | Description |
|---|---|---|
| `owner` | ✅ | |
| `repo` | ✅ | |
| `number` | ✅ | |

Outputs: `comments` — an array of comment objects with `body`, `id`, `user.login`, etc.

### `builtin.jq`

Runs a [jq](https://jqlang.github.io/jq/) expression against a previous step's output.

| Input | Required | Description |
|---|---|---|
| `input` | ✅ | Step id whose output is the jq input |
| `expr` | ✅ | jq expression |

If the expression returns a JSON object, its fields become the step's named outputs directly. Otherwise the result is available as `result`.

```yaml
# scalar output → {find.result}
- builtin.jq:
    id: find
    input: comments
    expr: 'first(.comments[].body | scan("CLOUDP-[0-9]+"))'

# object output → {find.key}, {find.url}
- builtin.jq:
    id: find
    input: comments
    expr: 'first(.comments[].body | scan("CLOUDP-[0-9]+")) | {key: .}'
```

`owner`, `repo`, and `number` for GitHub functions are typically interpolated from the payload:

```yaml
owner: "{payload.repository.owner.login}"
repo: "{payload.repository.name}"
number: "{payload.pull_request.number}"
```

---

## Adding an automation

1. Create `automations/my-automation.yaml`.
2. Add the repo to `deploy/eventsource.yaml` under the appropriate owner if not already listed.
3. Open a PR — Drone builds and deploys automatically on merge to `main`.

If the automation needs a new built-in function, add it to `src/functions/` in Rust and register it in `src/functions/mod.rs`.

## Onboarding a new repo

1. Add the repo to `deploy/eventsource.yaml` under the appropriate owner.
2. Add it to the `given.repos:` list in whichever `automations/*.yaml` files apply.
3. Open a PR — the EventSource will register the GitHub webhook automatically on deploy.

---

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

---

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
