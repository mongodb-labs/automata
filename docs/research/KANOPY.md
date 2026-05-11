# Kanopy — Infrastructure Research

> Source: https://kanopy.corp.mongodb.com/docs/ — audited 2026-05-11

Kanopy is MongoDB's internal self-service developer platform built on Kubernetes. It is the **target deployment infrastructure** for the automation hub.

---

## Core Components

| Component | Purpose | URL |
|---|---|---|
| Kubernetes (k8s) | Container orchestration | — |
| AWS ECR | Private container image registry | `795250896452.dkr.ecr.us-east-1.amazonaws.com` |
| Helm | Packaging & release management for k8s apps | `helm repo add mongodb https://10gen.github.io/helm-charts` |
| Drone | CI/CD pipelines | drone.corp.mongodb.com |
| Grafana | Dashboards & analytics | grafana.corp.mongodb.com (prod), grafana.staging.corp.mongodb.com |
| Prometheus | Monitoring & alerting | prometheus.prod.corp.mongodb.com |
| Splunk | Log aggregation | mongodb.splunkcloud.com |
| Jaeger (beta) | Distributed tracing | jaeger.corp.mongodb.com |
| Alertmanager | Alert routing | alertmanager.prod.corp.mongodb.com |

---

## Clusters

| Environment | API Server | Context name |
|---|---|---|
| Staging | `https://api.staging.corp.mongodb.com` | `api.staging.corp.mongodb.com` |
| Production | `https://api.prod.corp.mongodb.com` | `api.prod.corp.mongodb.com` |

---

## Helm Charts (relevant to automation hub)

Kanopy provides two official Helm charts from `https://10gen.github.io/helm-charts`:

### `mongodb/web-app` (chart version ≥ 4.25.0)
Deploys any Twelve-factor app: web services, REST/gRPC APIs, Slack/GitHub bots.
- Creates: Deployment, Service, Ingress, ConfigMap, Secret

### `mongodb/cronjobs` (chart version ≥ 1.9.0)
Deploys Kubernetes CronJobs for scheduled automation tasks:
- Pulling data from external APIs
- Performing maintenance on Jira projects
- Syncing data between services
- Creating and e-mailing analysis reports

**Critical gotcha**: if multiple repos deploy into the same namespace, each Helm `release` name must be unique — otherwise a deployment from one repo will overwrite all cronjobs from another.

---

## CI/CD: Drone Pipelines

All deployments go through Drone (`drone.corp.mongodb.com`). Pipelines live in `.drone.yml` at repo root.

**Pipeline type**: `kubernetes` (runs in a k8s pod). Default timeout: 60 minutes.

### Standard `.drone.yml` structure

```yaml
---
kind: pipeline
type: kubernetes
name: default
platform:
  arch: arm64          # arm64 recommended; amd64 also available
trigger:
  branch:
    - main
steps:
  # 1. Build & push container image to ECR
  - name: publish
    image: plugins/kaniko-ecr
    settings:
      create_repository: true
      registry: 795250896452.dkr.ecr.us-east-1.amazonaws.com
      repo: my-namespace/${DRONE_REPO_NAME}
      tags:
        - git-${DRONE_COMMIT_SHA:0:7}
        - latest
      access_key:
        from_secret: ecr_access_key
      secret_key:
        from_secret: ecr_secret_key
    when:
      event: [push]

  # 2. Deploy to staging via Helm
  - name: deploy-staging
    image: public.ecr.aws/kanopy/drone-helm:v3
    settings:
      chart: mongodb/cronjobs
      chart_version: 1.9.0
      add_repos: [mongodb=https://10gen.github.io/helm-charts]
      namespace: my-namespace
      release: automata-cronjobs
      values_files: ["environments/staging.yaml"]
      values: image.tag=git-${DRONE_COMMIT_SHA:0:7},...
      api_server: https://api.staging.corp.mongodb.com
      kubernetes_token:
        from_secret: staging_kubernetes_token
    when:
      event: [push]
```

### Monorepo path-based triggering

Use `paths` conditionals to run pipelines/steps only when specific files change:

```yaml
trigger:
  paths:
    include:
      - scripts/my-script/**
      - environments/staging.yaml
```

Essential for a monorepo — each automation can trigger independently.

### Advanced features

| Feature | How |
|---|---|
| Parallelization | Steps: same pod; Pipelines: different pods |
| Downstream triggers | `plugins/downstream` + `downstream_token` secret |
| Starlark scripting | Alternative to YAML for complex pipelines (`def main(ctx)`) |
| ARM64 | `platform: arch: arm64` (cold-start ~1-3 min if nodes scaled to 0) |
| Compute resources | `resources.requests` at pipeline level; `resources.limits` at step level |
| Helm dry-run | Add `dry_run: true` step before the deploy step for validation |

---

## Drone Secrets

Two types:

1. **Repository secrets** — stored encrypted in Drone DB, managed in Drone UI or via CLI. Referenced as `from_secret: <name>`.
   ```bash
   drone secret add <repo> --name=<name> --data=<value>
   ```

2. **Encrypted secrets** — generated with `drone encrypt <repo> <value>`, stored inline in `.drone.yml` as `kind: secret`.

### Standard secrets needed per repo

| Secret name | Value source | Purpose |
|---|---|---|
| `ecr_access_key` | `kubectl get secret ecr -o jsonpath="{.data.ecr_access_key}" \| base64 --decode` | Push images to ECR |
| `ecr_secret_key` | Same, `.ecr_secret_key` | Push images to ECR |
| `staging_kubernetes_token` | `kubectl get secret kanopy-cicd-token -o jsonpath="{.data.token}" \| base64 --decode` (staging context) | Deploy to staging |
| `prod_kubernetes_token` | Same (prod context) | Deploy to production |

---

## Kubernetes Secrets (App Secrets)

Managed with `ksec` (Helm plugin):

```bash
helm plugin install https://github.com/kanopy-platform/ksec

helm ksec set mysecret key1=value1 key2=value2   # create/update
helm ksec get mysecret                            # read
helm ksec push mysecret.env mysecret             # from env file
helm ksec pull mysecret mysecret.env             # to env file
```

Referenced in Helm values as `envFrom` or `secretRef` in the `web-app`/`cronjobs` chart.

---

## CronJob Operations

```bash
# List cronjobs in namespace
kubectl get cronjobs -n <namespace>

# Manually trigger a cronjob immediately
kubectl create job --from=cronjob/<cronjob-name> <job-name>-$(date | md5)

# View logs
kubectl logs <pod-name>
kubectl logs -f <pod-name>   # stream
```

---

## Observability

- **Logs**: All `stdout` output is automatically forwarded to Splunk. Avoid running `kubectl exec` with secrets in the command — output goes to Splunk.
- **Metrics**: Prometheus scrapes pods automatically if they expose a `/metrics` endpoint.
- **Dashboards**: Grafana for visualization.
- **Tracing**: Jaeger (beta) for distributed traces.
- **Health dashboards**: Production cluster at `grafana.corp.mongodb.com/d/DgX5qJmWz/kanopy-home-dashboard`

---

## Security: CorpSecure

All Kanopy services are protected behind CorpSecure (corporate authentication proxy at `login.corp.mongodb.com`) by default. Services requiring public access need explicit configuration. The automation hub will be internal-only, so default CorpSecure protection applies.

---

## Argo Workflows (beta)

> UI: workflows.staging.corp.mongodb.com / workflows.prod.corp.mongodb.com

Kubernetes-native workflow engine for orchestrating parallel jobs. Available in all Kanopy clusters (beta — don't depend on scaling for critical prod workloads).

**Key concepts:**
- **WorkflowTemplate** — reusable workflow definition (steps, containers, parameters)
- **CronWorkflow** — scheduled workflow (replaces k8s CronJobs for complex multi-step tasks)

**Managed via** `mongodb/argo-workflow-catalog` helm chart — store definitions in repo, deploy via Drone:
```bash
helm install automata-workflows mongodb/argo-workflow-catalog -f argo-workflow-catalog/values.yaml
```

**Defaults applied by Kanopy:**
- Workflows deleted after 1 day
- Pods deleted when workflow is deleted
- Max parallelism: 5 steps per workflow (overridable)
- Global cluster limit on simultaneous workflows

**Built-in Prometheus metrics** (via argo-workflow-catalog chart):
- `argo_workflows_{namespace}_runs_total`
- `argo_workflows_{namespace}_status` (Succeeded/Failed)
- `argo_workflows_{namespace}_duration`

**Data passing between steps:**
- < 256 kB: Output Parameters
- Larger: Volumes (PVC for workflow lifetime) or S3 Artifacts (bring your own bucket)

**RBAC requirement** — WorkflowTemplates using private ECR images must specify `command` explicitly and need a ServiceAccount with the executor RoleBinding (handled by argo-workflow-catalog chart ≥ v0.3.2 via `serviceAccounts` list).

---

## Argo Events (beta)

> Part of the same Argo ecosystem; controller installed in all Kanopy clusters.

Event-driven automation framework: detect events from external sources → trigger Argo Workflows or k8s objects.

**3 objects to create per event flow:**

| Object | Helm chart | Purpose |
|---|---|---|
| `EventBus` | `mongodb/argo-eventbus` | NATS JetStream message bus (one per namespace) |
| `EventSource` | manual (chart on roadmap) | Receives events from external systems |
| `Sensor` | manual (chart on roadmap) | Filters events and triggers downstream actions |

### GitHub as a well-known source

**GitHub is pre-approved** — no ENTSEC ticket needed. Self-service via annotation:

```yaml
apiVersion: argoproj.io/v1alpha1
kind: EventSource
metadata:
  name: automata-github
  namespace: <namespace>
  annotations:
    v1alpha1.argoslower.kanopy-platform/known-source: "github"
spec:
  eventBusName: automata-bus
  github:
    automata:
      repositories:
        - owner: mongodb
          names:
            - mongodb-atlas-cli
            # ... other repos
      webhook:
        endpoint: /push
        port: "12000"
        method: POST
      webhookSecret:
        name: automata-github-secret
        key: secret
      events:
        - "*"
      insecure: false
      active: true
      contentType: json
```

**Webhook URL format:**
```
https://webhooks.prod.corp.mongodb.com/<namespace>/<eventsource-name>/<endpoint>
```

**Requirements:**
- `WebhookSecret` must be ≥ 12 characters
- GitHub type EventSources must include a WebhookSecret

### Sensor (triggers Argo Workflow on event)

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Sensor
metadata:
  name: automata-sensor
spec:
  dependencies:
    - name: github-dep
      eventSourceName: automata-github
      eventName: automata
  triggers:
    - template:
        name: trigger-workflow
        argoWorkflow:
          operation: submit
          source:
            resource:
              apiVersion: argoproj.io/v1alpha1
              kind: WorkflowTemplate
              name: handle-pr-event
          parameters:
            - src:
                dependencyName: github-dep
                dataKey: body
              dest: spec.arguments.parameters.0.value
      rateLimit:
        unit: Second
        requestsPerUnit: 1   # Kanopy max; request exception if needed
```

---

## Support

- Slack: `#kanopy-users`
- Jira: https://jira.mongodb.org/servicedesk/customer/portal/48
- Docs: https://kanopy.corp.mongodb.com/docs/

---

## Automation Hub Deployment Model

For this monorepo, the expected pattern is:

```
automata/
├── .drone.yml                   # top-level pipeline, path-filtered per automation
├── environments/
│   ├── staging.yaml             # Helm values for staging
│   └── prod.yaml                # Helm values for production
├── scripts/                     # automation scripts packaged into the container image
├── Dockerfile                   # single image for all cron-based automations
└── cronjobs.yml                 # Helm cronjobs chart values (one job per automation)
```

Each automation becomes a separate `CronJob` entry in `cronjobs.yml`. The Drone pipeline rebuilds and redeploys on push to `main`, using path filters to avoid unnecessary builds.

Helm release naming: use a single unique release name (e.g. `automata-cronjobs`) for all jobs — since this is a monorepo, there's only one Helm release per namespace, avoiding the multi-repo collision gotcha.
