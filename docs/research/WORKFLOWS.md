# APIx DevTools — GitHub Actions Workflow Research

> Audited: 2026-05-11  
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
| mongodb-mcp-server | mongodb-js | TypeScript | 21 |
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
