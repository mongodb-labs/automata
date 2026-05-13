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

> Part of the Argo ecosystem; controller installed in all Kanopy clusters. **Independent of Argo Workflows** — can trigger HTTP endpoints, k8s objects, or Argo Workflows.

Event-driven automation framework: detect events from external sources → trigger downstream actions.

**3 objects to create per event flow:**

| Object | Helm chart | Purpose |
|---|---|---|
| `EventBus` | `mongodb/argo-eventbus` | NATS JetStream message bus (one per namespace) |
| `EventSource` | manual (chart on roadmap) | Receives events from external systems |
| `Sensor` | manual (chart on roadmap) | Filters events and triggers downstream actions |

**Sensor trigger types (relevant):**

| Type | Use |
|---|---|
| `http` | POST to any HTTP endpoint — **not subject to the k8s rate limit** |
| `argoWorkflow` | Submit an Argo Workflow |
| `k8s` | Create/patch a k8s resource — max 1/s on Kanopy |

### External GitHub webhooks: all three resources required

To receive GitHub webhooks from outside the cluster you need **exactly three resources**. Missing any one causes a silent failure (403, DNS failure, or connection reset):

#### 1. EventSource

Must include a `service.ports` block so argoslower creates the external-facing Service and URL. Without it, no Service is created and the webhook URL never becomes reachable.

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
  service:
    ports:
      - port: 80
        targetPort: 12000        # must match webhook.port below
  github:
    automata:
      repositories:
        - owner: mongodb
          names:
            - my-repo
      webhook:
        endpoint: /github
        port: "12000"
        method: POST
      webhookSecret:
        name: automata-secrets
        key: GITHUB_WEBHOOK_SECRET   # must be ≥ 12 characters
      events:
        - "*"
      insecure: false
      active: true
      contentType: json
```

**External webhook URL:**
```
https://webhooks.staging.corp.mongodb.com/<namespace>/<eventsource-name>/<endpoint>
# e.g. https://webhooks.staging.corp.mongodb.com/skunkworks/automata-github/github
```

#### 2. ServiceCatalogEntry

Creates the Istio AuthorizationPolicy that allows GitHub's IP ranges through the mesh. Without this, every request gets **403 from `istio-envoy`** before the EventSource pod even sees it.

```yaml
apiVersion: service.kanopy-platform.github.io/v1beta1
kind: ServiceCatalogEntry
metadata:
  name: automata-github
  namespace: skunkworks
spec:
  authorization:
    allowVanity: true   # this is what tells argoslower to open the external path
    groups: []
    scopes: []
  selector:
    matchLabels:
      controller: eventsource-controller
      eventsource-name: automata-github
      owner-name: automata-github
```

#### 3. EventBus

Must have `streamConfig: "replicas: 1"` set **at install time** — this is **immutable**. The default `replicas: 3` causes CrashLoopBackOff on Kanopy's single-node NATS: `nats: replicas > 1 not supported in non-clustered mode`.

```yaml
# eventbus-values.yaml
jetstreams:
  automata-bus:
    version: 2.10.10
    replicas: 1
    streamConfig: |
      replicas: 1
```

**If you need to change streamConfig on an existing EventBus** (immutable field — `helm upgrade` won't touch it):
```bash
helm uninstall automata-eventbus -n skunkworks
# EventBus CRD gets stuck with a finalizer — force remove it:
kubectl patch eventbus/automata-bus -n skunkworks --type=json \
  -p '[{"op":"remove","path":"/metadata/finalizers"}]'
helm install automata-eventbus mongodb/argo-eventbus \
  -f deploy/eventbus-values.yaml -n skunkworks
```

### Sensor: Istio sidecar injection required

The sensor pod **must** have the Istio sidecar injected. Without it the sensor sends plaintext HTTP; the target service's Istio sidecar enforces mTLS and resets the connection (`read: connection reset by peer`).

Add this to the Sensor spec — it makes the sensor a mesh participant so mTLS works and same-namespace traffic is allowed:

```yaml
spec:
  eventBusName: automata-bus
  template:
    metadata:
      annotations:
        sidecar.istio.io/inject: "true"
  dependencies: ...
```

When sidecar injection is working the sensor pod shows `2/2` containers (app + `istio-proxy`). Without it: `1/1`.

No extra AuthorizationPolicy is needed — Kanopy allows same-namespace mTLS traffic by default once the sidecar is injected.

### Sensor: correct headers and payload format

`headers` under the `http` trigger is a **map**, not an array:

```yaml
# correct
http:
  headers:
    Content-Type: application/json

# wrong — causes deploy errors
http:
  headers:
    - name: Content-Type
      value: application/json
```

### Sensor: body arrives as a JSON-encoded string

When sensor payload extraction uses `dataKey: body`, the body field in the envelope posted to your service arrives as a **JSON-encoded string**, not a JSON object. You must parse it:

```rust
// envelope["body"] is a serde_json::Value::String, not Value::Object
let payload = if let Some(s) = body_value.as_str() {
    serde_json::from_str::<serde_json::Value>(s)?
} else {
    body_value
};
```

### Sensor: full working example

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Sensor
metadata:
  name: automata-sensor
  namespace: skunkworks
spec:
  eventBusName: automata-bus
  template:
    metadata:
      annotations:
        sidecar.istio.io/inject: "true"
  dependencies:
    - name: github-dep
      eventSourceName: automata-github
      eventName: automata
  triggers:
    - template:
        name: call-automata
        http:
          url: http://automata-web-app.skunkworks.svc.cluster.local/webhook/github/argo
          method: POST
          headers:
            Content-Type: application/json
          payload:
            - src:
                dependencyName: github-dep
                dataKey: headers.X-Github-Event.0
              dest: github_event
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

### Service DNS naming

The `mongodb/web-app` Helm chart names the Kubernetes Service `<release>-web-app`, not `<release>`. The cluster-internal DNS is:
```
http://<release>-web-app.<namespace>.svc.cluster.local
# e.g. http://automata-web-app.skunkworks.svc.cluster.local
```

### CorpSecure: do not use the web-app service as a webhook target

Services deployed with `mesh: enabled: true` are behind CorpSecure (Okta SSO). External systems (GitHub, etc.) will get a **302 redirect to `login.corp.mongodb.com`** — they cannot authenticate. Always use the argoslower EventSource URL for external webhooks, and use the sensor `http` trigger for internal calls to the web-app.

**Rate limits:**
- `k8s` triggers: Kanopy enforces max 1/s; request exception via KANOPY ticket if needed
- `http` triggers: **no Kanopy-enforced rate limit**

---

## Support

- Slack: `#kanopy-users`
- Jira: https://jira.mongodb.org/servicedesk/customer/portal/48
- Docs: https://kanopy.corp.mongodb.com/docs/

---

## Ingress: Mesh-Only Namespaces

All new namespaces (including `skunkworks`) are **mesh-only** — Istio service mesh, no Traefik. Kubernetes `Ingress` resources are rejected by the admission controller in these namespaces.

```
# Check if namespace is Traefik-enabled (empty = mesh-only)
kubectl get namespace <ns> -o jsonpath='{.metadata.labels.kanopy-platform\.github\.io/traefik-enabled}'
```

To expose a service externally in a mesh-only namespace, set `ingress.enabled: true` with `hosts` and `mesh: enabled: true`. **Do not define a custom `services` block** — the chart's default service naming (`<release>-web-app`) is what the generated VirtualService routes to. A custom `services` entry appends the port number to the name (e.g. `<release>-web-app-80`), which the VirtualService won't find, causing 503s.

```yaml
ingress:
  enabled: true
  hosts:
    - <release>.<namespace>.staging.corp.mongodb.com

mesh:
  enabled: true
```

The `ingress.hosts` field sets the hostname for the Istio VirtualService. The hostname follows the pattern:
- Staging: `automata.skunkworks.staging.corp.mongodb.com`
- Production: `automata.skunkworks.prod.corp.mongodb.com`

Traffic routes through the Istio user ingress gateway (`ig-u.staging.corp.mongodb.com`). All mesh hostnames are HTTPS-only and protected by CorpSecure (Okta).

---

## Automation Hub Deployment Model

automata runs as a long-lived HTTP server deployed via `mongodb/web-app`. Argo Events delivers GitHub webhooks to it via a Sensor `http` trigger.

```
automata/
├── .drone.yml                   # build image → deploy web-app + eventbus + kubectl apply
├── Dockerfile                   # multi-stage: cargo build → distroless/cc-debian12 (glibc required)
├── Cargo.toml
├── src/                         # Rust source
├── automations/                 # declarative automation YAML files
└── deploy/
    ├── staging.yaml             # mongodb/web-app Helm values for staging
    ├── eventbus-values.yaml     # mongodb/argo-eventbus Helm values
    ├── eventsource.yaml         # GitHub EventSource (kubectl apply, no Helm chart)
    ├── eventsource-sce.yaml     # ServiceCatalogEntry — opens external webhook path via argoslower
    └── sensor.yaml              # Sensor with http trigger (kubectl apply, no Helm chart)
```

Helm releases in `skunkworks` namespace:
- `automata` — the web service (`mongodb/web-app`)
- `automata-eventbus` — the NATS event bus (`mongodb/argo-eventbus`)

EventSource and Sensor are applied via `kubectl apply` (no Helm chart available yet per Kanopy roadmap).

**Gotchas discovered in practice:**
- Use `distroless/cc-debian12` not `distroless/static` — Rust binaries are dynamically linked to glibc by default.
- Pass the automations directory as a CLI argument: `ENTRYPOINT ["/automata", "/automations"]`. The binary defaults to CWD (`.`) which is unpredictable in a container.
- Do not set a custom `services` block in web-app values — it renames the service and breaks the VirtualService routing (503). Let the chart use its defaults.
- `platform: arch: amd64` in `.drone.yml` — skunkworks nodes are amd64; arm64 produces `exec format error`.
- Set `RUST_LOG=info` (or more specific filter) in `staging.yaml` env — `tracing`'s `EnvFilter::from_default_env()` filters everything out if the env var is missing, producing zero log output.
- Sensor pod should show `2/2` containers when Istio sidecar injection is working. If it shows `1/1`, the `sidecar.istio.io/inject: "true"` annotation is missing from `spec.template.metadata.annotations`.
