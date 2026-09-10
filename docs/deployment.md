# Production deployment

- Private repository: [Jotrorox/calc](https://github.com/Jotrorox/calc)
- API base URL: https://calc-production-c962.up.railway.app
- Health: https://calc-production-c962.up.railway.app/health
- Calculator: `POST https://calc-production-c962.up.railway.app/calc`
- Railway dashboard: https://railway.com/project/fb38367a-c5ab-41ed-9d66-2e93207cf1b7/service/87c990bb-94b0-4f83-8bcd-1abbf5fa42bd?environmentId=bc94ead0-5102-4336-b495-714012601d16
- Environment: `production`
- Source branch: `main`

The initial deployment was uploaded through Railway CLI 5.52.0 using the `use-railway` skill. Deployment `5a1678dd-26b4-4dbf-8117-39b1e52faf27` reached `SUCCESS` on 2026-09-10. Its public health endpoint returned `{"status":"ok"}` and the context calculation `f(5)` followed by `ans + f(3)` for `f(x)=x^2` returned `34` at 256-bit precision.

The production configuration uses a Dockerfile build, the `/health` readiness path, a 30-second readiness timeout, and `ON_FAILURE` restarts with at most 3 retries. Settings are persisted in Railway and reproducible with:

```sh
bash scripts/configure-railway.sh \
  fb38367a-c5ab-41ed-9d66-2e93207cf1b7 \
  bc94ead0-5102-4336-b495-714012601d16 \
  87c990bb-94b0-4f83-8bcd-1abbf5fa42bd \
  Jotrorox/calc
```

To inspect releases, use explicit scope:

```sh
railway deployment list \
  --project fb38367a-c5ab-41ed-9d66-2e93207cf1b7 \
  --environment bc94ead0-5102-4336-b495-714012601d16 \
  --service 87c990bb-94b0-4f83-8bcd-1abbf5fa42bd --json
```

The public API does not require credentials. GitHub repository access remains private. No deployment tokens or account secrets are committed.
