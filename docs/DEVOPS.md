# TokenHarbor2API — Pipeline CI/CD DevOps

## 1. Vue d''ensemble (ASCII)

```
                    ┌──────────────────────────────────────────────────────┐
                    │                  GitHub Actions                       │
                    └──────────────────────────────────────────────────────┘
 PR / push develop / push main / tag v*
        │
        ▼
   ┌─────────┐   ┌──────────┐   ┌─────────┐   ┌──────────┐   ┌─────────────┐
   │  BUILD   │──▶│ QUALITÉ  │──▶│  TESTS  │──▶│ SÉCURITÉ │──▶│ NOTIFICATIONS│
   │ cargo    │   │ fmt+clip │   │ unit+   │   │ audit+   │   │ (échec→     │
   │ build    │   │ py -Dwar │   │ coverage│   │ gitleaks │   │  Slack/gh)  │
   └─────────┘   └──────────┘   └─────────┘   └──────────┘   └─────────────┘
        │              │              │             │              ▲
        ▼              ▼              ▼             ▼              │
   artifact exe   ┌─────────┐   ┌─────────┐   ┌──────────┐         │
   (upload)       │ DEPLOY  │   │  CI OK  │   │ Chefcks  │         │
                  │ staging │   └─────────┘   └──────────┘         │
                  │ develop │                                       │
                  └────┬────┘                                       │
                       ▼                                            │
                  ┌──────────┐  approbation ┌──────────────┐        │
                  │ PRODUCTION│──▶ manuelle ─▶  déploiement │        │
                  │ main/tag  │◀── rollback ◀── réel        │        │
                  └──────────┘               └──────────────┘        │
```

## 2. Branches & déclencheurs

| Branche    | Build | Qualité | Tests | Sécurité | Deploy |
|------------|-------|---------|-------|----------|--------|
| feature/*  | PR → oui | oui | oui | oui | non |
| develop    | oui    | oui     | oui   | oui      | **staging (auto)** |
| main       | oui    | oui     | oui   | oui      | **production (approbation)** |
| tag v*     | oui    | oui     | oui   | oui      | production + Release |

## 3. Workflows

| Fichier | Rôle |
|---------|------|
| `.github/workflows/ci.yml` | CI complet (build/qualité/tests/sécurité/notifications) |
| `.github/workflows/deploy.yml` | Staging (develop) + Production (main, approbation) + rollback |
| `.github/workflows/docker.yml` | Image GHCR multi-arch (amd64+arm64) |

## 4. Secrets & variables à configurer

### GitHub Secrets (Settings → Secrets and variables → Actions)
| Secret | Obligatoire | Usage |
|--------|-------------|-------|
| `SLACK_WEBHOOK_URL` | non (défaut: échec visible dans l''UI) | Notifications d''échec |
| `DOCKER_USERNAME` / `DOCKER_PASSWORD` | non (GHCR via GITHUB_TOKEN) | Registry externe optionnel |
| `SSH_HOST` / `SSH_KEY` | si déploiement réel | Déploiement VPS/Docker |

### GitHub Environments (Settings → Environments)
| Environment | Protection |
|-------------|-----------|
| `staging` | — (auto) |
| `production` | **Required reviewers** (approbation manuelle) + wait timer |

### Variables d''environnement
| Variable | Défaut | Usage |
|----------|--------|-------|
| `LISTEN_ADDR` | `127.0.0.1:47830` | Port gateway |
| `UPSTREAM_BASE_URL` | `https://tokenharbor.ai` | URL amont |
| `HTTP_PROXY` | — | Proxy (dev) |
| `AUTH_TOKENS` / `API_KEYS` | — | Credentials |

## 5. Bonnes pratiques Rust/GitHub Actions

- **Cache**: `actions/cache` sur `~/.cargo/registry` + `target`, clé = hash `Cargo.lock`
- **Concurrence**: `concurrency.cancel-in-progress` pour annuler les runs PR obsolètes
- **Toolchain**: épingler `1.95.0` (pas `stable`) pour repro
- **Clippy**: `-D warnings` (zéro tolérance)
- **Couverture**: `cargo-llvm-cov --fail-under-lines 20` (baseline réelle: 20.4%)
- **Release**: `softprops/action-gh-release` sur tag `v*`
- **Docker**: buildx multi-arch + cache `type=gha`

## 6. Dépannage

| Erreur | Cause | Solution |
|--------|-------|----------|
| `LINK : fatal error LNK1104` | exe verrouillé par process | stopper le process, relancer |
| `error: failed to remove file` | fichier target verrouillé | `cargo clean` ou stopper service |
| `clippy -D warnings` échoue | lint mineur | `cargo clippy --fix` puis commit |
| `cargo audit` signale CVE | dépendance vulnérable | `cargo update -p <crate>` |
| Gitleaks détecte secret | vrai secret ou faux positif | corriger / ajouter allowlist |
| Run Docker échoue | buildx multi-arch | activer `docker/setup-buildx-action` |
| Approbation prod bloquée | reviewer requis | Settings → Environments → approbation |

## 7. Rollback

- **Manuel**: `workflow_dispatch` avec `rollback: true` → job `rollback`
- **Image**: chaque push tagué `$sha` → repo GHCR garde l''historique

## 8. Limites connues (honnêtes)

- Le déploiement réel (SSH/ECS/K8s) nécessite des credentials que je ne peux pas configurer pour vous — le workflow fournit le squelette (`docker compose` / healthcheck) et les steps d''approbation.
- La vérification E2E complète nécessite un runner GitHub Actions (irremplaçable en local) — les workflows sont poussés et déclenchés pour preuve.
