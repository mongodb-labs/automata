# APIx DevTools — GitHub Actions Workflow Research

> Audited: 2026-05-11 (Jira fields section updated: 2026-05-15)
> Scope: 16 repositories, ~120 workflow files  
> Groups: AtlasCLI, Atlas Local, MCP & Internal, Misc

---

## Repositories

### AtlasCLI Group
| Repo | Org | Language | Workflows |
|---|---|---|---|
| mongodb-atlas-cli | mongodb | Go | 15 |
| atlas-cli-core | mongodb | Go | 2 |
| atlas-github-action | mongodb | Shell/JS | 4 |
| chocolatey-packages | mongodb-forks | PowerShell | 2 |
| (atlas-cli plugins) | — | — | — |

### Atlas Local Group
| Repo | Org | Language | Workflows |
|---|---|---|---|
| mongodb-atlas-local | mongodb | Go/Docker | 11 |
| atlas-local-lib | mongodb | Rust | 6 |
| atlas-local-cli | mongodb | Rust | 8 |
| atlas-local-lib-js | mongodb-js | Rust/NAPI-RS/TS | 6 |

### MCP & Internal Group
| Repo | Org | Language | Workflows |
|---|---|---|---|
| mongodb-mcp-server | mongodb-js | TypeScript | 21 | **excluded from automata scope** |
| apix-bot | 10gen | TypeScript | 2 |
| apix-dashboards | 10gen | — | 0 |
| apix-devtools | 10gen | — | 0 |

### Misc Group
| Repo | Org | Language | Workflows |
|---|---|---|---|
| apix-action | mongodb | TypeScript | 8 |
| digest | mongodb-forks | Go | 2 |
| cobra2snooty | mongodb-labs | Go | 3 |
| openapi | mongodb | Go/TypeScript | 25 |

---

## Internal Action Library: `mongodb/apix-action`

The foundational composite action library used across all repos with Jira integration.

| Sub-action | Purpose |
|---|---|
| `apix-action/create-jira` | Create a Jira issue with custom fields, components, labels |
| `apix-action/find-jira` | Find an existing Jira issue by JQL query |
| `apix-action/transition-jira` | Transition issue status (resolve, reopen, won't fix) |
| `apix-action/comment-jira` | Add a comment to a Jira issue |
| `apix-action/token` | Mint an ApixBot GitHub App token (bypasses branch protections) |

**Testing approach**: Each sub-action is integration-tested via an Nginx+Docker mock server that simulates the Jira REST API. Dist files are verified to be in sync after every dependency bump.

---

## Pattern 1: Jira Lifecycle Automation

The most pervasive pattern across the estate.

### The `auto_close_jira` Pipeline

Used in: `atlas-cli`, `mongodb-atlas-local`, `cobra2snooty`, `atlas-github-action`

```
PR opened (Dependabot or automation)
  → create-jira (CLOUDP Story)
  → post Jira URL as PR comment
  → label PR 'auto_close_jira'
  → auto-merge (squash) via ApixBot token

PR merged (if labeled 'auto_close_jira')
  → close-jira.yml fires
  → transition-jira (transition ID 1381 → resolve/fixed)
```

**Ticket key lookup strategy in `close-jira.yml`:**
1. If branch name starts with `CLOUDP-`, use branch name as ticket key
2. Otherwise, grep PR comments for a `CLOUDP-NNNNN` pattern posted by the bot earlier

**Jira custom fields used consistently:**
- `customfield_12751` — team assignment (value from `vars.JIRA_TEAM_APIX_2`)
- `customfield_10257` — sprint/epic linkage
- Component: `AtlasCLI`
- `fixVersions` set on creation

**Transition IDs:**
- `1381` — Resolve / Fixed
- `1371` — Close / Won't Fix
- `1351` — Reopen

### Issue-to-Jira Sync

`atlas-cli` (`issues.yml`): every new GitHub Issue → creates a `CLOUDP` Jira Story automatically.

`mongodb-mcp-server` (`jira-issue.yml`): every new GitHub Issue → creates an MCP-project Jira ticket; on issue close → transitions Jira to closed.

`atlas-github-action` (`jira-issue.yml`): full 3-way lifecycle — open/close/reopen synced to Jira.

### Failure-Triggered Jira Tickets

`openapi` (`failure-handler.yml`): on release pipeline failure, creates a Jira ticket AND a GitHub issue. Deduplicates by checking for existing open GH issue with same title first.

`openapi` (`api-versions-reminder.yml`): creates a CLOUDP Jira ticket for each API approaching sunset horizon (1 week, 1 month, 3 months).

`openapi` (`release-IPA-metrics.yml`): creates Jira tickets for IPA warning-level spec violations via `handle_warning_violations.sh`.

---

## Pattern 2: ApixBot GitHub App Token

All elevated operations use the internal GitHub App rather than a PAT or `GITHUB_TOKEN`.

```yaml
- uses: mongodb/apix-action/token@<pinned-SHA>
  with:
    app-id: ${{ secrets.APIXBOT_APP_ID }}
    private-key: ${{ secrets.APIXBOT_APP_PEM }}
```

Used for: bypass branch protections, bot commits, auto-merge, cross-repo PRs.

Repos using it: `apix-action`, `cobra2snooty`, `atlas-cli`, `mongodb-atlas-local`, `atlas-local-cli`, `apix-bot`.

---

## Pattern 3: Dependabot Automation

### Full Pipeline (Go repos with Jira)
Repos: `atlas-cli`, `mongodb-atlas-local`, `cobra2snooty`

```
Dependabot opens PR
  → regenerate purls.txt (PURL list for SSDLC)
  → regenerate THIRD_PARTY_NOTICES
  → create CLOUDP Jira Story
  → post Jira link as PR comment
  → label PR 'auto_close_jira'
  → auto-merge (squash) via ApixBot
  → on merge: resolve Jira ticket
```

### Merge-Only (no Jira)
Repos: `apix-action`, `apix-bot`, `atlas-local-lib`, `atlas-local-cli`, `atlas-local-lib-js`

```
Dependabot opens PR
  → auto-approve
  → auto-merge (squash) via ApixBot or GITHUB_TOKEN
```

### Dist Rebuild on Dependency Bump
Repo: `apix-action` (`dependabot-update-dist.yaml`)

When Dependabot bumps a JS dependency, rebuilds `verify-changed-files/dist` and `create-jira/dist`, then commits and pushes the refreshed artifacts using the bot token.

---

## Pattern 4: PR Title Enforcement

| Approach | Repos |
|---|---|
| `realm/ci-actions/title-checker` — conventional commit + `CLOUDP-XXXX` required in title | `atlas-local-lib`, `atlas-local-cli`, `atlas-local-lib-js`, `mongodb-atlas-local` |
| `amannn/action-semantic-pull-request` — conventional commit, no Jira required | `mongodb-mcp-server`, `openapi` |
| Custom shell script | `atlas-cli` |
| Scoped format `type(scope): subject` with `ipa`/`prod` scopes | `openapi` |

---

## Pattern 5: SSDLC Compliance

Present in: `atlas-cli`, `mongodb-atlas-local`

### `generate-augmented-sbom.yml`
1. Silkbomb generates a Software Bill of Materials (SBOM)
2. Kondukto augments it with additional metadata
3. Triggered on PR and on release

### `update-ssdlc-report.yaml/.yml`
- Generates the SSDLC compliance report
- Bot-commits it to `master`/`main` on release

### PURL Regeneration
`dependabot-update-purls.yml` / `dependabot-purls.yml`: regenerates `build/package/purls.txt` (PURL list) when Dependabot bumps a dependency.

---

## Pattern 6: Code Health / CI by Language

### Go
- `golangci/golangci-lint-action` (version varies — see divergences section)
- `actions/setup-go` + module cache
- Cross-platform matrix: ubuntu / macos / windows
- `make test` or `go test ./...`
- `actionlint` on workflow YAML files (openapi only)

### Rust
- `cargo fmt` + `cargo clippy` + `cargo test`
- `cargo deny` — license and dependency policy enforcement
- `cargo audit` — CVE scanning (also runs as weekly cron via `security-audit.yml`)
- `cargo udeps` — unused dependency detection
- Coveralls parallel upload: `lcov-result-merger` merges coverage, `parallel-finished` job signals completion

### TypeScript / JavaScript
- `oxlint` or `eslint` + `prettier`
- Jest test suites
- Coveralls (parallel pattern in `atlas-local-lib-js`)
- CodeQL (JS/TS + Actions) in `mongodb-mcp-server` and `atlas-local-lib-js`

### NAPI-RS (atlas-local-lib-js)
- Multi-platform binary builds: ubuntu / macos-x86 / macos-arm / windows
- Docker-based Linux binding integration tests
- Dual lint pipeline: oxlint (JS) + clippy (Rust)

---

## Pattern 7: Release Automation

### cargo-dist (atlas-local-cli)
Multi-platform Rust binary builds + GitHub Release. Artifacts are GPG-signed and Windows `.exe` is GRS/Garasign signed.

### cargo publish (atlas-local-lib)
OIDC trusted publishing to crates.io. No static credentials.

### npm publish with provenance (atlas-local-lib-js, openapi IPA)
`NODE_AUTH_TOKEN` via OIDC. Includes npm provenance attestation.

### GoReleaser (openapi/foascli)
Cross-platform Go binary builds triggered by a GPG-signed tag.

### git-cliff / orhun/git-cliff-action
Changelog generation from conventional commits.  
Used in: `atlas-local-lib`, `mongodb-mcp-server`, `mongodb-atlas-local`.

### MCP Registry (mongodb-mcp-server)
OIDC publish via `rust-lang/crates-io-auth-action` + `mcp-publisher` action. Also publishes `.mcpb` manifest file.

### Docker
Multi-arch push to Docker Hub. Signed with cosign (atlas-cli) or GRS/Garasign (atlas-cli Windows variant).

### Release PR pattern (prepare-release.yml)
Repos: `atlas-local-lib`, `atlas-local-cli`, `mongodb-mcp-server`

```
workflow_dispatch or cron
  → bump version in manifest/Cargo.toml/package.json
  → generate changelog via git-cliff
  → open PR with changes
  → PR merge triggers actual release
```

---

## Pattern 8: Code Signing

### GRS/Garasign (Windows EXE signing)
Used in: `atlas-cli` (Docker workflow), `atlas-local-cli` (`sign-zip.yml`)

```
OIDC → assume AWS ECR role
→ pull GRS Docker image
→ jsign the .exe with MongoDB code signing cert
```

### GPG artifact signing
Used in: `atlas-local-cli` (all release artifacts), `openapi` (git tags for foascli and IPA ruleset)

Secrets: `GPG_PRIVATE_KEY`, `PASSPHRASE`  
Action: `rickstaa/action-create-tag` with gpg params

### cosign container signing
Used in: `atlas-cli` (signs Docker images after push to Docker Hub)

---

## Pattern 9: Bot-Authored PRs

`peter-evans/create-pull-request` is the standard for automation PRs.

| Trigger | Action | Repo |
|---|---|---|
| Weekly cron | Bump Go SDK version | atlas-cli |
| Weekly cron | Bump OpenAPI spec | atlas-cli |
| Daily cron | Update Atlas Local manifest | mongodb-atlas-local |
| On release | Bump `AtlasLocalPluginMinVersion` in atlas-cli | atlas-local-cli |
| Post-release | Cleanup upcoming→stable OAS files | openapi |

All such PRs are labeled `auto_close_jira` so the associated Jira ticket resolves on merge.

---

## Pattern 10: Idempotent PR Comments

`marocchino/sticky-pull-request-comment` — updates the same comment on re-runs instead of posting duplicates.

Used in: `atlas-cli` (docs team review notification).

---

## Pattern 11: OIDC Authentication (No Static Credentials)

| Service | Auth | Repos |
|---|---|---|
| AWS S3 | `aws-actions/configure-aws-credentials` with `role-to-assume` | openapi |
| crates.io | Cargo trusted publishing | atlas-local-lib |
| npm registry | OIDC token exchange | atlas-local-lib-js, openapi (IPA) |
| MCP Registry | `rust-lang/crates-io-auth-action` | mongodb-mcp-server |
| AWS ECR (GRS signing) | OIDC role assumption | atlas-cli, atlas-local-cli |

All AWS interactions require `id-token: write` permission on the job.

---

## Pattern 12: Scheduled Automations

| Schedule | Action | Repo |
|---|---|---|
| Weekly (Mon) | Bump Go SDK version → PR | atlas-cli |
| Weekly (Mon) | Bump OpenAPI spec → PR | atlas-cli |
| Weekly (Mon) | Sync IPA guidelines from mongodb/ipa | apix-bot |
| Weekly (Mon 09:00 UTC) | Changelog digest → Slack thread | openapi |
| Weekly (Mon 09:00 UTC) | API sunset reminders (3m/1m/1w horizons) | openapi |
| Daily | Rebuild Docker image | atlas-cli, mongodb-mcp-server |
| Daily (weekdays) | Check Atlas Local manifest, open update PR if stale | mongodb-atlas-local |
| Daily (11:00 UTC) | IPA metrics collection → S3 + Slack alerts for violations | openapi |
| Every 2h (Mon–Fri) | OpenAPI spec release pipeline (dev/qa/staging/prod) | openapi |
| Weekly | Snyk dependency monitor | atlas-cli-core |
| Weekly | `cargo audit` CVE scan | atlas-local-lib, atlas-local-cli, atlas-local-lib-js |

---

## Pattern 13: Failure Handling

### openapi (most sophisticated)
Centralized `failure-handler.yml` called by all release sub-workflows:
1. Search for existing open GitHub Issue with same title (deduplicate)
2. Create GitHub Issue from template (`JasonEtco/create-an-issue`) if not found
3. Run `create_jira_ticket.sh` to open a CLOUDP Jira ticket
4. Comment on GH issue with Jira link

Combined with `retry-handler.yml`:
- Auto-retry up to 3 attempts before escalating to failure-handler

### mongodb-mcp-server
Creates a GitHub Issue directly when the nightly Docker build fails (`docker.yml`).

---

## Pattern 14: Reusable `workflow_call` Architecture

### openapi (most decomposed estate in the team)

Call graph:
```
release-spec-runner (bi-hourly cron or manual)
  └─ release-spec (per env: dev / qa / staging / prod)
       ├─ generate-openapi          ← OIDC S3 download + FOASCLI federated spec
       ├─ required-spec-validations ← Spectral lint + atlas-sdk-go pipeline smoke test (BLOCKING)
       ├─ optional-spec-validations ← IPA Spectral + Postman Spectral (non-blocking, retried)
       ├─ release-changelog         ← git-auto-commit changelog to branch
       ├─ release-postman           ← (prod only) convert to Postman collection + upload
       ├─ generate-bump-pages       ← dynamic matrix → Bump.sh API doc deploy
       ├─ release-cleanup           ← PR to delete upcoming files after stable promotion
       ├─ retry-handler             ← gh run rerun --failed, up to 3 attempts
       └─ failure-handler           ← GH issue + Jira ticket if retries exhausted
```

### mongodb-mcp-server
`docker-publish.yml` — reusable multi-arch Docker build/push workflow.  
Called from `publish.yml` for releases and `docker.yml` for the nightly cron.

---

## Pattern 15: Stale Management

Identical `stale.yml` across product repos:
- 30 days inactive → stale label
- 30 more days → close
- Exempt label: `not_stale`

Repos: `atlas-cli`, `atlas-github-action`, `mongodb-mcp-server`

---

## Pattern 16: Slack Notifications

All use direct Slack REST API (`chat.postMessage`) with `SLACK_BEARER_TOKEN`, not a Slack GitHub Action.

| Notification | Trigger | Repo |
|---|---|---|
| Docs team tagged as reviewer | PR review request | atlas-cli |
| Weekly changelog digest | Monday 09:00 UTC cron | openapi |
| API sunset reminder (3m/1m/1w) | Monday 09:00 UTC cron | openapi |
| IPA warning violations | Daily metrics run | openapi |

---

## Pattern 17: `GitHubSecurityLab/actions-permissions/monitor@v1`

Runtime CI permissions monitoring. Controlled by `vars.PERMISSIONS_CONFIG`.

Present in: `apix-action` (all 8 workflows), `cobra2snooty`.  
**Not yet adopted** across the wider estate.

---

## Divergences & Inconsistencies

| Issue | Detail |
|---|---|
| **golangci-lint version drift** | v2.1.6 (atlas-cli-core) vs v2.10.1 (atlas-cli) vs v9.2.0 (digest, cobra2snooty) vs v2.11.4 (openapi) |
| **Action pinning strategy** | `openapi` pins every third-party action to full commit SHA; all other repos use semver tags (`@v4`, `@v6`) |
| **Permissions monitoring** | `actions-permissions/monitor` only in `apix-action` + `cobra2snooty`, not broadly adopted |
| **SSDLC compliance** | Only `atlas-cli` and `mongodb-atlas-local` have Silkbomb/Kondukto SBOM generation |
| **Jira ticket in PR title** | Required by Atlas Local repos and `atlas-cli` custom check, but not by `openapi`, `apix-action`, or `digest` |
| **Secret name casing** | `openapi` uses lowercase (`jira_api_token`, `api_bot_pat`); all others use uppercase (`JIRA_API_TOKEN`, `APIXBOT_APP_PEM`) |
| **CodeQL** | Only `mongodb-mcp-server` and `atlas-local-lib-js` have CodeQL scanning |

---

## Secrets & Variables Inventory

### Secrets

| Secret | Repos |
|---|---|
| `APIXBOT_APP_ID` / `APIXBOT_APP_PEM` | atlas-cli, mongodb-atlas-local, atlas-local-cli, cobra2snooty, apix-action, apix-bot |
| `JIRA_API_TOKEN` / `jira_api_token` | atlas-cli, mongodb-atlas-local, atlas-github-action, cobra2snooty, openapi |
| `ASSIGNEE_JIRA_TICKET` | atlas-cli, cobra2snooty |
| `GPG_PRIVATE_KEY` / `PASSPHRASE` | atlas-local-cli, openapi |
| `SLACK_BEARER_TOKEN` | atlas-cli, openapi |
| `SLACK_CHANNEL_ID` / `SLACK_CHANNEL_ID_APIX_PLATFORM_DEV` | atlas-cli, openapi |
| `SLACK_APIX_PLATFORM_ONCALL_USER` | openapi |
| `NPM_TOKEN` / `IPA_VALIDATION_NPM_TOKEN` | atlas-local-lib-js, openapi |
| `BUMP_TOKEN` / `bump_token` | openapi |
| `POSTMAN_API_KEY` / `WORKSPACE_ID` | openapi |
| `IPA_S3_BUCKET_DW_PROD_PREFIX` | openapi |
| `MMS_DEPLOYED_SHA_URL_PROD` | openapi |
| `SNYK_TOKEN` | atlas-cli-core |
| `API_BOT_PAT` / `api_bot_pat` | openapi |

### Variables

| Variable | Repos |
|---|---|
| `PERMISSIONS_CONFIG` | apix-action, cobra2snooty |
| `JIRA_TEAM_APIX_2` | atlas-cli, mongodb-atlas-local, cobra2snooty |
| `JIRA_TEAM_ID_APIX_PLATFORM` | openapi |
| `FOASCLI_VERSION` | openapi |
| `AWS_DEFAULT_REGION` / `AWS_S3_ROLE_TO_ASSUME` / `S3_BUCKET_*` | openapi |
| `ATLAS_ADMIN_V1_DOC_ID` / `ATLAS_ADMIN_V2_DOC_ID_*` | openapi |
| `ATLAS_PROD_BASE_URL` | openapi |
| `IPA_METRIC_COLLECTION_AWS_S3_ROLE_TO_ASSUME_PROD` | openapi |

---

## Third-Party Actions Used

| Action | Purpose | Repos |
|---|---|---|
| `golangci/golangci-lint-action` | Go linting | atlas-cli, atlas-cli-core, digest, cobra2snooty, openapi |
| `peter-evans/create-pull-request` | Bot-authored PRs | atlas-cli, mongodb-atlas-local, openapi |
| `marocchino/sticky-pull-request-comment` | Idempotent PR comments | atlas-cli |
| `orhun/git-cliff-action` | Changelog from conventional commits | atlas-local-lib, mongodb-mcp-server, mongodb-atlas-local |
| `rickstaa/action-create-tag` | GPG-signed git tags | openapi |
| `goreleaser/goreleaser-action` | Go binary releases | openapi (foascli) |
| `bump-sh/github-action` | API docs deployment to Bump.sh | openapi |
| `amannn/action-semantic-pull-request` | Conventional commit PR title check | mongodb-mcp-server, openapi |
| `realm/ci-actions/title-checker` | Conventional commit + Jira ticket in PR title | atlas-local-* |
| `stefanzweifel/git-auto-commit-action` | Bot auto-commit without PR | openapi |
| `aws-actions/configure-aws-credentials` | OIDC-based AWS auth | openapi, atlas-cli |
| `JasonEtco/create-an-issue` | Create GH issue from template | openapi |
| `GitHubSecurityLab/actions-permissions/monitor` | Runtime permissions auditing | apix-action, cobra2snooty |
| `stoplightio/spectral-action` | OpenAPI Spectral linting | openapi |
| `peter-evans/create-or-update-comment` | PR comment create/update | cobra2snooty |

---

## Jira Fields Per Repo — Automata Integration Audit

> Scope: all repos in `environments/common/eventsource.yaml`
> Purpose: determine exact Jira field values needed per repo before enabling automata automations
> `jira-lifecycle-close.yaml` removed 2026-05-15 (duplicate of close path already in `jira-lifecycle-atlascli.yaml`)

### Repos with existing GitHub Actions Jira automation

These repos already run Jira workflows via GHA. The automata automations are intended to replace or mirror them.

#### `mongodb/mongodb-atlas-cli`

| Trigger | Field | Value |
|---|---|---|
| Dependabot PR open | project | `CLOUDP` |
| | component | `AtlasCLI` |
| | issuetype | `Story` |
| | summary | `AtlasCLI Dependency Update n. {number}` |
| | assignee | `${{ secrets.ASSIGNEE_JIRA_TICKET }}` |
| | fixVersions | name: `"next-atlascli-release"` (**by name**) |
| | customfield_12751 | id: `"22223"` (from `library_owners_jira.json`, always this value) |
| | customfield_10257 | id: `"11861"` |
| GitHub Issue open | project | `CLOUDP` |
| | component | `AtlasCLI` |
| | issuetype | `Story` |
| | summary | `HELP: GitHub Issue n. {number}` |
| | assignee | `${{ vars.ASSIGNEE_JIRA_TICKET }}` |
| | fixVersions | name: `"Not Applicable"` |
| | customfield_12751 | id: `"22223"` |
| | customfield_10257 | id: `"11861"` |
| Transition — merged | transition_id | `1381` (Resolved) |
| Transition — reopen | transition_id | `1351` (Reopened) |
| Ticket scan pattern | regex | `CLOUDP-[0-9]+` (branch name prefix or PR comment grep) |

#### `mongodb/atlas-github-action`

Identical to atlas-cli issue workflow; no Dependabot→Jira path in GHA.

| Trigger | Field | Value |
|---|---|---|
| GitHub Issue open | project | `CLOUDP` |
| | component | `AtlasCLI` |
| | issuetype | `Story` |
| | summary | `HELP: GitHub Issue n. {number}` |
| | assignee | `${{ vars.ASSIGNEE_JIRA_TICKET }}` |
| | fixVersions | name: `"Not Applicable"` |
| | customfield_12751 | id: `"22223"` |
| | customfield_10257 | id: `"11861"` |
| Transition — close | transition_id | `1381` (Resolved) |
| Transition — reopen | transition_id | `1351` (Reopened) |
| Ticket lookup | JQL | `project = CLOUDP AND description ~ '<issue_url>'` |

#### `mongodb/mongodb-atlas-local`

| Trigger | Field | Value |
|---|---|---|
| Dependabot PR open | project | `CLOUDP` |
| | component | `local-atlas-experience` (**different from AtlasCLI**) |
| | issuetype | `Story` |
| | summary | `LocalDev Dependency Update n. {number}` |
| | assignee | `${{ secrets.JIRA_ASSIGNEE }}` (different secret name) |
| | fixVersions | id: `"17641"` (numeric ID, **different from atlas-cli**) |
| | customfield_12751 | id: `"22223"` |
| | customfield_10257 | id: `"11861"` |
| Transition — merged | transition_id | `1381` (Resolve Issue / Fixed) |
| Transition — not merged | transition_id | `1371` (Close Issue / Won't Fix) |
| Ticket scan pattern | regex | `CLOUDP-[0-9]+` |

#### `mongodb-labs/cobra2snooty`

| Trigger | Field | Value |
|---|---|---|
| Dependabot PR open | project | `CLOUDP` |
| | component | `AtlasCLI` |
| | issuetype | `Story` |
| | summary | `cobra2snooty Dependency Update n. {number}` |
| | assignee | `${{ secrets.ASSIGNEE_JIRA_TICKET }}` |
| | fixVersions | id: `"41805"` (**different from all other repos**) |
| | customfield_12751 | `${{ vars.JIRA_TEAM_APIX_2 }}` (variable, not hardcoded — same team ID `"22223"` at runtime) |
| | customfield_10257 | id: `"11861"` |
| Transition — merged only | transition_id | `1381` (Resolved / Fixed) |
| | | **No "Won't Fix" path** (unlike atlas-local) |
| Ticket scan pattern | regex | `CLOUDP-[0-9]+` |

#### `mongodb-js/mongodb-mcp-server`

**Excluded from automata scope** — shared with other teams, uses a separate `MCP` Jira project with different field IDs and transition IDs. Removed from `eventsource.yaml` 2026-05-15. Retained here for reference only.

| Trigger | Field | Value |
|---|---|---|
| GitHub Issue open | project | `MCP` |
| | component | *(none)* |
| | issuetype | `Bug` |
| | customfield_12751 | ids: `"27247"` and `"27326"` |
| | customfield_10257 | *(not set)* |
| Transition — close | transition_id | `61` |
| Ticket lookup | JQL | `project = MCP AND description ~ '<issue_url>'` |

---

### Repos with NO Jira automation

These repos either have no Jira workflows at all or are not plausible candidates.

| Repo | Has Dependabot | Notes |
|---|---|---|
| `mongodb/atlas-local-lib` | Yes (cargo + github-actions, weekly Mon) | Rust library; no Jira in GHA |
| `mongodb/atlas-local-cli` | Yes (cargo only, weekly Mon) | Rust CLI; no Jira in GHA |
| `mongodb-js/atlas-local-lib-js` | Yes (cargo + npm + github-actions, weekly Mon) | NAPI-RS; no Jira in GHA |
| `mongodb/apix-action` | Yes (github-actions + npm ×2, weekly) | IS the Jira action library; only mock values in CI tests |
| `mongodb/atlas-cli-core` | Yes (gomod + github-actions, weekly Tue; reviewer: `apix-2`) | No Jira in GHA |
| `mongodb/atlas-cli-plugin-example` | No | Single release workflow only |
| `10gen/apix-dashboards` | N/A | Repo inaccessible / does not exist at this path |
| `mongodb-forks/chocolatey-packages` | No | Chocolatey test + publish only |
| `mongodb-forks/digest` | Yes (gomod + github-actions, weekly Tue) | Go lib; no Jira in GHA |

---

### GitHub Issue sync — per-repo analysis

Both repos that have issue→Jira automation in GHA use **identical** Jira parameters, so no `builtin.lookup` is needed in `issue-sync-atlascli.yaml`.

| Field | `mongodb/mongodb-atlas-cli` | `mongodb/atlas-github-action` |
|---|---|---|
| project | `CLOUDP` | `CLOUDP` |
| component | `AtlasCLI` | `AtlasCLI` |
| issuetype | `Story` | `Story` |
| summary | `HELP: GitHub Issue n. {number}` | `HELP: GitHub Issue n. {number}` |
| assignee | `vars.ASSIGNEE_JIRA_TICKET` | `vars.ASSIGNEE_JIRA_TICKET` |
| fixVersions | name: `"Not Applicable"` | name: `"Not Applicable"` |
| customfield_12751 | id: `"22223"` | id: `"22223"` |
| customfield_10257 | id: `"11861"` = `value: "Not Needed"` | id: `"11861"` = `value: "Not Needed"` |
| Transition — close | `1381` (Resolved) | `1381` (Resolved) |
| Transition — reopen | `1351` (Reopened) | `1351` (Reopened) |
| Ticket lookup strategy | JQL `project = CLOUDP AND description ~ '<url>'` | JQL `project = CLOUDP AND description ~ '<url>'` |

Automata uses comment scanning instead of JQL (controls what it posts) — equivalent outcome.

`mongodb-atlas-local`, `cobra2snooty`, and all other repos have no issue→Jira automation in GHA.

#### All other repos — Jira field values inferred from existing issues

All repos belong to the same team. `customfield_12751 id: "22223"` = display name **"APIx DevTools"** — consistent across all repos. `fixVersions` for issue-sync should always be `Not Applicable` (no version target for a general GitHub issue).

| Repo | project | component | customfield_12751 | fixVersions | Notes |
|---|---|---|---|---|---|
| `mongodb/mongodb-atlas-local` | `CLOUDP` | `local-atlas-experience` | `22223` | `Not Applicable` | Already in lifecycle; no issue sync in GHA |
| `mongodb/atlas-local-lib` | `CLOUDP` | `local-atlas-experience` | `22223` | `Not Applicable` | CLOUDP-397846 uses both AtlasCLI + local-atlas-experience; primary is local |
| `mongodb/atlas-local-cli` | `CLOUDP` | `local-atlas-experience` | `22223` | `Not Applicable` | Same pattern as atlas-local-lib |
| `mongodb-js/atlas-local-lib-js` | `CLOUDP` | `local-atlas-experience` | `22223` | `Not Applicable` | CLOUDP-362273 shows APIx DevTools; component inferred from sibling repos |
| `mongodb/atlas-cli-core` | `CLOUDP` | `AtlasCLI` | `22223` | `Not Applicable` | CLOUDP-400902 uses AtlasCLI + `next-atlascli-release` but that's a feature ticket; issues use `Not Applicable` |
| `mongodb-labs/cobra2snooty` | `CLOUDP` | `AtlasCLI` | `22223` | `Not Applicable` | CLOUDP-300844, CLOUDP-138384; already in lifecycle |
| `mongodb-forks/chocolatey-packages` | `CLOUDP` | `AtlasCLI` | `22223` | `Not Applicable` | CLOUDP-318987; Windows packaging for AtlasCLI |
| `mongodb/apix-action` | — | — | — | — | Action library — issues here are dev tasks, not user-facing; **not a candidate** |
| `mongodb-forks/digest` | — | — | — | — | Upstream Go lib fork — no user-facing issues expected; **not a candidate** |

> `atlas-local-*` repos use `local-atlas-experience` as their primary component (consistent with `mongodb-atlas-local` lifecycle config). Jira issues sometimes also carry `AtlasCLI` as a secondary component but automata's `jira.create_issue` takes a single component.

**Cross-check with Dependabot PR creation:** the same component mapping applies to `jira-lifecycle-cloudp.yaml`. `atlas-local-lib`, `atlas-local-cli`, and `atlas-local-lib-js` are now explicit entries in both lookup tables — without them the default (`AtlasCLI`) would silently misclassify their tickets.

| Repo | component | lookup table entry |
|---|---|---|
| `mongodb-atlas-cli` | `AtlasCLI` | explicit (`fix_version_name: next-atlascli-release`) |
| `mongodb-atlas-local` | `local-atlas-experience` | explicit |
| `atlas-local-lib` | `local-atlas-experience` | explicit |
| `atlas-local-cli` | `local-atlas-experience` | explicit |
| `atlas-local-lib-js` | `local-atlas-experience` | explicit |
| `cobra2snooty` | `AtlasCLI` | explicit |
| `atlas-cli-core` | `AtlasCLI` | covered by default |
| `chocolatey-packages` | `AtlasCLI` | covered by default |
| *(any new repo)* | `AtlasCLI` | covered by default |

---

### Known transition IDs in CLOUDP

| ID | Meaning | Used for |
|---|---|---|
| `1381` | Resolved / Fixed | PR merged; issue closed |
| `1371` | Close / Won't Fix | PR closed without merge only |
| `1351` | Reopened | Issue reopened |

---

### Discrepancies fixed (2026-05-15)

#### `jira-lifecycle-cloudp.yaml` (renamed from `jira-lifecycle-atlascli.yaml`)

| Field | Old value | Fixed value | Note |
|---|---|---|---|
| `customfield_10257` | `id: "11861"` | `value: "Not Needed"` | Both formats valid; name confirmed via Jira API (`id: "11861"` = `"Not Needed"`) |
| `fixVersions` (atlas-cli) | `id: "50180"` | `name: "next-atlascli-release"` | ID `50180` resolves to this name — standardised on names |
| `fixVersions` (atlas-local) | `id: "17641"` | `name: "Not Applicable"` | ID `17641` resolves to `"Not Applicable"` |
| `fixVersions` (cobra2snooty) | `id: "41805"` | `name: "Not Applicable"` | ID `41805` was `"atlascli-1.40.0"` — stale released version, corrected |
| per-repo config | 3 separate pipeline steps | single `builtin.lookup` keyed on `payload.repository.name` | `component` + `fix_version_name` per repo; `default` covers unknown repos |
| `jira-lifecycle-close.yaml` | existed as separate file | deleted | Duplicate of close/transition logic already in lifecycle file |

#### `issue-sync-atlascli.yaml`

| Field | Old value | Fixed value | Note |
|---|---|---|---|
| `fixVersions` | `id: "50180"` | `name: "Not Applicable"` | Issues use "Not Applicable"; `50180` is the Dependabot rolling version |
| Transition — closed | `"1371"` (Won't Fix) | `"1381"` (Resolved) | GHA uses 1381 for issue close; 1371 is for unmerged PRs only |
| Transition — reopened | `"3"` (unknown) | `"1351"` (Reopened) | Confirmed from atlas-cli `issues.yml` |

---

### Automata automation coverage per repo (current state)

| Repo | `jira-lifecycle-cloudp` | `issue-sync-atlascli` | `dependabot-approve/automerge` |
|---|---|---|---|
| `mongodb/mongodb-atlas-cli` | commented (ready) | commented (ready) | commented (ready) |
| `mongodb/mongodb-atlas-local` | commented (ready) | — not applicable | commented (ready) |
| `mongodb/atlas-github-action` | — not applicable | commented (ready) | — not applicable |
| `mongodb-labs/cobra2snooty` | commented (ready) | — not applicable | not in list yet |
| `mongodb-js/mongodb-mcp-server` | **excluded** | **excluded** | **excluded** |
| `mongodb/atlas-local-lib` | — | — | commented (ready) |
| `mongodb/atlas-local-cli` | — | — | — not in list |
| `mongodb-js/atlas-local-lib-js` | — | — | commented (ready) |
| `mongodb/apix-action` | — | — | commented (ready) |
| `mongodb/atlas-cli-core` | — | — | — not in list |
| `mongodb/atlas-cli-plugin-example` | — | — | — |
| `mongodb-forks/chocolatey-packages` | — | — | — |
| `mongodb-forks/digest` | — | — | — |
