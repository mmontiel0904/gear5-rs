# gear5-rs

Rust workspace that scrapes the official One Piece Card Game card list, stores the catalogue in PostgreSQL, and serves an API-key-gated HTTP API.

- `gear5-core` — shared domain model, scraper, key crypto.
- `gear5-api` — `axum` HTTP server with an embedded daily scrape scheduler.
- `gear5-cli` (`gear5`) — admin CLI for migrations, manual scrapes, and API-key lifecycle.

## Quick start (local dev)

```bash
# 1. Local Postgres
createdb gear5

# 2. Apply migrations & seed scrape
export DATABASE_URL=postgres://gear5:gear5@localhost/gear5
cargo run -p gear5-cli -- migrate
cargo run -p gear5-cli -- scrape run

# 3. Issue an API key (shown once)
cargo run -p gear5-cli -- key create --name dev --scopes read

# 4. Boot the API
cargo run -p gear5-api
```

## API surface

| Path                  | Auth   | Notes                                                 |
|-----------------------|--------|-------------------------------------------------------|
| `GET /health`         | none   | Liveness                                              |
| `GET /health/scrape`  | none   | Last scrape status; 503 when stale                    |
| `GET /sets`           | `read` | All known sets                                        |
| `GET /cards`          | `read` | Filters: `set`, `color`, `category`, `rarity`, `q`, … |
| `GET /cards/{code}`   | `read` | Single card                                           |
| `GET /dump`           | `read` | NDJSON snapshot of every card                         |
| `GET /images/{file}`  | `read` | Card art served from the local image directory        |
| `POST /admin/keys`    | `admin`| Issue a new key (returns plaintext once)              |
| `GET /admin/keys`     | `admin`| List keys (hashes redacted)                           |
| `DELETE /admin/keys/{id}` | `admin` | Revoke                                          |
| `POST /admin/scrape/run`  | `admin` | Trigger an on-demand scrape                     |

Authentication: `Authorization: Bearer op_live_<...>`.

## Configuration

Layered via `figment`:

1. `deploy/config.example.toml` documents every field.
2. `GEAR5_*` environment variables override (double underscore separates sections, e.g. `GEAR5_SERVER__BIND`).
3. `DATABASE_URL` must come from the environment.

See `.env.example`.

---

## Push to GitHub

From this working tree:

```bash
git add -A
git commit -m "init gear5-rs"
gh repo create gear5-rs --private --source=. --remote=origin --push
# or, with a remote you create in the UI:
# git remote add origin git@github.com:<you>/gear5-rs.git
# git branch -M main
# git push -u origin main
```

The repo ignores `target/`, `.env`, and `/var`, `/data`, `/images` working directories.

---

## Deployment — Ubuntu server (systemd)

Target: a fresh Ubuntu 22.04 / 24.04 VM, owns its own DB and disk, no public traffic yet (Cloudflare Tunnel handles ingress — see next section).

The sequence below has dependencies wired in deliberately: packages → database → service user / paths → binaries → environment file (now valid because DB exists) → one-shot CLI tasks → systemd unit. Run top-to-bottom.

### 1. Install all OS packages

```bash
sudo apt update
sudo apt install -y \
    build-essential pkg-config libssl-dev ca-certificates curl git jq \
    postgresql postgresql-contrib
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version   # expect 1.80+
```

`postgresql-contrib` ships `pg_trgm`; `pgcrypto` is part of core Postgres. Both are referenced by `migrations/20260516000001_init.sql`.

### 2. Start Postgres and pick a password

The `postgresql` service is enabled on install. Confirm it, then set a strong password to put inside `DATABASE_URL`. Use any method you like — example with `openssl`:

```bash
sudo systemctl enable --now postgresql
sudo systemctl status postgresql --no-pager | head -n 5

PG_PW=$(openssl rand -base64 24)
echo "Postgres password for the gear5 role: $PG_PW"
# Copy this somewhere safe; you will paste it into gear5.env in step 6.
```

### 3. Create the database, role, and extensions

`pgcrypto` and `pg_trgm` need superuser to install. The cleanest path is to run them once as the `postgres` superuser, then own the database with a non-privileged `gear5` role that the app will log in as.

```bash
sudo -u postgres psql <<SQL
CREATE ROLE gear5 LOGIN PASSWORD '$PG_PW';
CREATE DATABASE gear5 OWNER gear5;
\c gear5
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
SQL
```

Quick verification — confirm the role can log in over the loopback interface using password auth (this is the same path the app will use):

```bash
PGPASSWORD="$PG_PW" psql -h 127.0.0.1 -U gear5 -d gear5 \
    -c "SELECT extname FROM pg_extension WHERE extname IN ('pgcrypto','pg_trgm');"
```

If you get `FATAL: password authentication failed`, your `pg_hba.conf` is set to `peer` for local connections. Edit `/etc/postgresql/<ver>/main/pg_hba.conf`, change the `host all all 127.0.0.1/32 ident` (or `peer`) line to `scram-sha-256`, then `sudo systemctl reload postgresql`.

### 4. Create the service user and directories

The API runs as a system user with no shell. Its home directory holds the image cache, and `/etc/gear5` holds the env file.

```bash
sudo useradd --system --create-home --home-dir /var/lib/gear5 \
    --shell /usr/sbin/nologin gear5
sudo install -d -o gear5 -g gear5 /var/lib/gear5/images
sudo install -d -o root  -g gear5 /etc/gear5
```

### 5. Clone, build, and stage the binaries

```bash
cd ~
git clone https://github.com/<you>/gear5-rs.git
cd gear5-rs
cargo build --release --bins
sudo install -m 0755 target/release/gear5-api /usr/local/bin/gear5-api
sudo install -m 0755 target/release/gear5     /usr/local/bin/gear5
```

### 6. Write the environment file (uses the password from step 2)

```bash
sudo tee /etc/gear5/gear5.env > /dev/null <<EOF
DATABASE_URL=postgres://gear5:$PG_PW@127.0.0.1:5432/gear5
GEAR5_SERVER__BIND=127.0.0.1:8080
GEAR5_IMAGES__DIR=/var/lib/gear5/images
GEAR5_SCRAPE__ENABLED=true
GEAR5_SCRAPE__RUN_AT_STARTUP=true
GEAR5_SCRAPE__CRON_HOUR_UTC=4
GEAR5_SCRAPE__USER_AGENT=gear5-rs/0.1 (+contact-on-request)
GEAR5_SCRAPE__STALE_AFTER_HOURS=36
RUST_LOG=info,gear5_api=info,gear5_core=info
EOF
sudo chown root:gear5 /etc/gear5/gear5.env
sudo chmod 0640      /etc/gear5/gear5.env
```

Binding `127.0.0.1` means the API is only reachable on loopback. Cloudflare Tunnel reaches it from inside the host; nothing else does.

### 7. Apply migrations, seed the first scrape, issue an admin key

Each `gear5` subcommand reads the same env file. The shell snippet below sources it for one command at a time and runs as the `gear5` user:

```bash
run_as_gear5() {
    sudo -u gear5 bash -c "set -a; . /etc/gear5/gear5.env; set +a; $*"
}

run_as_gear5 /usr/local/bin/gear5 migrate
run_as_gear5 /usr/local/bin/gear5 scrape run
run_as_gear5 /usr/local/bin/gear5 key create --name admin --scopes read,admin --rate 600
# Save the plaintext key now — it is shown once.
```

The first scrape pulls ~25 set pages plus a few thousand card images, so expect it to take a minute or two with the configured polite jitter.

### 8. Install and start the systemd unit

```bash
sudo install -m 0644 deploy/gear5-api.service /etc/systemd/system/gear5-api.service
sudo systemctl daemon-reload
sudo systemctl enable --now gear5-api
sudo systemctl status gear5-api --no-pager
journalctl -u gear5-api -n 50 --no-pager
```

Smoke test on the host:

```bash
curl -s http://127.0.0.1:8080/health
curl -s http://127.0.0.1:8080/health/scrape | jq
curl -s -H "Authorization: Bearer <your-key>" \
    http://127.0.0.1:8080/cards/OP01-001 | jq
```

### 9. Upgrade workflow

```bash
cd ~/gear5-rs
git pull
cargo build --release --bins
sudo install -m 0755 target/release/gear5-api /usr/local/bin/gear5-api
sudo install -m 0755 target/release/gear5     /usr/local/bin/gear5
sudo systemctl restart gear5-api
```

Migrations run automatically on each `gear5-api` start, so a fresh binary picks up new schema without a manual step.

### 10. Backups

The catalog is rebuildable from a clean scrape, but API keys, custom scopes, and scrape history are not. Back up the Postgres database; skip the image directory.

```bash
sudo -u postgres pg_dump -Fc gear5 > /var/backups/gear5-$(date +%F).dump
```

Optional `systemd` timer (`/etc/systemd/system/gear5-backup.service` + `.timer`) can run this daily. Keep ~7 days on disk and ship one weekly off-box.

---

## Deployment — Cloudflare

Two viable paths. **Cloudflare Tunnel is the recommended one** for a self-hosted Ubuntu VM with no static public IP, no inbound port forwarding, and free TLS at the edge.

### Option A — Cloudflare Tunnel (recommended)

The tunnel daemon `cloudflared` runs on the same VM, dials *outbound* to Cloudflare, and Cloudflare routes `https://api.example.com` traffic into your local `127.0.0.1:8080`. Nothing on the VM needs to listen on a public port.

Prereqs: a domain already on Cloudflare DNS, and `gear5-api` already running on `127.0.0.1:8080` (Step 4 above).

#### 1. Install cloudflared

```bash
curl -L --output cloudflared.deb \
    https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
sudo dpkg -i cloudflared.deb
cloudflared --version
```

#### 2. Authenticate and create the tunnel

```bash
cloudflared tunnel login                      # opens a URL; pick the zone in the Cloudflare UI
cloudflared tunnel create gear5-api           # writes ~/.cloudflared/<UUID>.json (the tunnel credentials)
cloudflared tunnel route dns gear5-api api.example.com
```

The DNS route adds a proxied CNAME `api.example.com → <UUID>.cfargotunnel.com` for you.

#### 3. Tunnel config

Put credentials and config in `/etc/cloudflared` so `systemd` can manage the tunnel as a service.

```bash
sudo mkdir -p /etc/cloudflared
sudo cp ~/.cloudflared/<UUID>.json /etc/cloudflared/

sudo tee /etc/cloudflared/config.yml > /dev/null <<'EOF'
tunnel: gear5-api
credentials-file: /etc/cloudflared/<UUID>.json

ingress:
  - hostname: api.example.com
    service: http://127.0.0.1:8080
    originRequest:
      connectTimeout: 10s
      noTLSVerify: false
  - service: http_status:404
EOF
```

#### 4. Run as a systemd service

```bash
sudo cloudflared service install
sudo systemctl enable --now cloudflared
sudo systemctl status cloudflared
journalctl -u cloudflared -f
```

#### 5. End-to-end test

```bash
curl -s https://api.example.com/health
curl -s -H "Authorization: Bearer <your-key>" https://api.example.com/cards/OP01-001 | jq
```

#### 6. Hardening at the Cloudflare edge

In the Cloudflare dashboard for `api.example.com`:

- **SSL/TLS → Overview**: set encryption mode to **Full (strict)**. Tunnel terminates HTTPS inside the tunnel, so origin verification is automatic.
- **Security → WAF**: leave the Cloudflare Managed Ruleset on. Optionally add a custom rule that rate-limits `/admin/*` paths regardless of API key.
- **Caching → Cache Rules**: cache `GET /images/*` aggressively (the URL is versioned, immutable). Optionally cache `GET /sets` for a few minutes. Do **not** cache anything under `/cards/*` or `/admin/*`.
- **Rules → Page Rules / Configuration Rules**: disable browser integrity check on `/health` so external uptime probes still get a fast 200/503.
- **Zero Trust → Access (optional)**: gate `/admin/*` behind a Cloudflare Access policy (email OTP, GitHub login, etc.) to add a second factor in front of the admin scope.

### Option B — Cloudflare DNS in front of a public origin

Only worth it if your VM already has a static public IP and a real TLS cert (e.g. via `caddy` or `nginx` + `certbot`) terminating on port 443.

1. Point an `A` record `api.example.com → <public-ip>` with the orange cloud (proxied) on.
2. SSL/TLS mode: **Full (strict)** if you have a valid origin cert, **Full** if self-signed.
3. Open only ports 80 + 443 on the VM firewall; restrict by Cloudflare IP ranges for extra hardening (see <https://www.cloudflare.com/ips/>).
4. Same cache / WAF tips as Option A apply.

> Workers / Pages are **not** a deployment target for this project — `gear5-api` is a native binary that owns its Postgres connection and embedded scheduler; it does not fit the Workers runtime model.

---

## Operational checklist

- `journalctl -u gear5-api -f` — live API and scheduler logs.
- `journalctl -u cloudflared -f` — tunnel state.
- `curl -s http://127.0.0.1:8080/health/scrape | jq` — last scrape status. 503 means stale (>36 h) or 3 consecutive failures.
- `gear5 scrape status` — last 10 scrape runs with counts.
- `gear5 key list` — issued API keys with last-used timestamps.
- `gear5 key rotate <prefix>` — revoke + reissue with the same scopes.
