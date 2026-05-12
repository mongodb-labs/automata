# Contributing to automata

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Docker](https://docs.docker.com/get-docker/) with Compose v2.22+
- [gh CLI](https://cli.github.com/) authenticated with your GitHub account
- [kubectl](https://kubernetes.io/docs/tasks/tools/) configured against the staging cluster — follow the [Kanopy docs](https://kanopy.corp.mongodb.com/docs/) to set up access, then verify with `kubectl get ns skunkworks`
- [ngrok](https://ngrok.com/) v3 with an account and auth token (free tier works) — config file at `~/.config/ngrok/ngrok.yml`

## Local setup

### 1. Pull secrets from Kubernetes

```bash
GITHUB_APP_PRIVATE_KEY=$(kubectl get secret automata-secrets -n skunkworks \
  -o jsonpath='{.data.GITHUB_APP_PRIVATE_KEY}' | base64 -d | awk '{printf "%s\\n", $0}' | tr -d '\n')

cat > .env <<EOF
GITHUB_APP_ID=$(kubectl get secret automata-secrets -n skunkworks -o jsonpath='{.data.GITHUB_APP_ID}' | base64 -d)
GITHUB_APP_PRIVATE_KEY=${GITHUB_APP_PRIVATE_KEY}
GITHUB_WEBHOOK_SECRET=$(kubectl get secret automata-secrets -n skunkworks -o jsonpath='{.data.GITHUB_WEBHOOK_SECRET}' | base64 -d)
SENSOR_TOKEN=$(kubectl get secret automata-secrets -n skunkworks -o jsonpath='{.data.SENSOR_TOKEN}' | base64 -d)
JIRA_BASE_URL=$(kubectl get secret automata-secrets -n skunkworks -o jsonpath='{.data.JIRA_BASE_URL}' | base64 -d)
JIRA_API_TOKEN=$(kubectl get secret automata-secrets -n skunkworks -o jsonpath='{.data.JIRA_API_TOKEN}' | base64 -d)
NGROK_AUTHTOKEN=$(grep authtoken ~/.config/ngrok/ngrok.yml | awk '{print $2}')
EOF
```

> `GITHUB_APP_PRIVATE_KEY` must be a single line with literal `\n` separating the PEM lines. The `awk` command above handles this.

`.env` is gitignored — never commit it.

### 2. Start the stack

```bash
docker compose watch
```

This builds and starts automata and an ngrok tunnel. `docker compose watch` monitors `automations/` for YAML changes and restarts automata automatically — no rebuild needed.

Get the public ngrok URL:

```bash
curl -s http://localhost:4040/api/tunnels | jq -r '.tunnels[0].public_url'
```

### 3. Register the GitHub webhook

```bash
NGROK_URL=$(curl -s http://localhost:4040/api/tunnels | jq -r '.tunnels[0].public_url')
WEBHOOK_SECRET=$(grep GITHUB_WEBHOOK_SECRET .env | cut -d= -f2)

gh api repos/mongodb-labs/automata/hooks \
  --method POST \
  -F "name=web" \
  -F "active=true" \
  -F "events[]=pull_request" \
  -F "events[]=issues" \
  -F "config[url]=${NGROK_URL}/webhook/github/raw" \
  -F "config[content_type]=json" \
  -F "config[secret]=${WEBHOOK_SECRET}"
```

The ngrok URL changes every time the ngrok container restarts (free plan). Re-run this command after each `docker compose watch` restart:

```bash
# Update an existing webhook
HOOK_ID=$(gh api repos/mongodb-labs/automata/hooks --jq '.[0].id')
NGROK_URL=$(curl -s http://localhost:4040/api/tunnels | jq -r '.tunnels[0].public_url')

gh api repos/mongodb-labs/automata/hooks/${HOOK_ID} \
  --method PATCH \
  -F "config[url]=${NGROK_URL}/webhook/github/raw"
```

## Testing automations locally

### Issue lifecycle

```bash
# Open an issue → automata creates a Jira ticket
ISSUE=$(gh issue create -R mongodb-labs/automata --title "Test: $(date)" --body "local test")
ISSUE_NUMBER=$(echo $ISSUE | grep -o '[0-9]*$')

# The GitHub App currently lacks issues:write — post the Jira comment manually:
# Check docker compose logs for the Jira URL, then:
gh issue comment $ISSUE_NUMBER -R mongodb-labs/automata --body "Jira ticket: <url-from-logs>"

# Close the issue → automata transitions the Jira ticket to Closed
gh issue close $ISSUE_NUMBER -R mongodb-labs/automata

# Reopen → automata transitions back to Open
gh issue reopen $ISSUE_NUMBER -R mongodb-labs/automata
```

Watch logs:

```bash
docker compose logs -f automata
```

### PR label trigger (`create_jira`)

Adding the `create_jira` label to any PR in `mongodb-labs/automata` fires Jira ticket creation and removes the label afterwards.

```bash
PR_NUMBER=<number>
gh pr edit $PR_NUMBER -R mongodb-labs/automata --add-label create_jira
```

### Sending a raw event without a real webhook

For quick iteration you can POST directly to the raw endpoint, signing the body yourself:

```bash
PAYLOAD='{"action":"opened","repository":{"full_name":"mongodb-labs/automata","name":"automata","owner":{"login":"mongodb-labs"}},"issue":{"number":99,"title":"synthetic test"},"sender":{"login":"test-user"}}'
SECRET=$(grep GITHUB_WEBHOOK_SECRET .env | cut -d= -f2)
SIG="sha256=$(echo -n "$PAYLOAD" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')"

curl -X POST http://localhost:8080/webhook/github/raw \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: issues" \
  -H "X-Hub-Signature-256: $SIG" \
  -d "$PAYLOAD"
```

## Running tests

```bash
cargo test
```

## Editing automation YAML

Automation files live in `automations/`. Changes are picked up without a rebuild when running `docker compose watch` — automata restarts automatically within a few seconds of saving.

## Adding a new built-in function

1. Implement it in `src/functions/<namespace>.rs` following the existing pattern — takes `inputs: &HashMap<String, serde_yaml::Value>` and `ctx: &ExecutionContext`, returns `anyhow::Result<serde_json::Value>`.
2. Register it in `src/functions/mod.rs` in the `dispatch` match.
3. If it needs a new API client, add the client under `src/<namespace>/mod.rs` and wire it into `AppState` in `src/state.rs`.
4. Document it in `README.md` under "Built-in functions".

## Endpoints

See the [Endpoints section in README.md](README.md#endpoints) for the full list.
