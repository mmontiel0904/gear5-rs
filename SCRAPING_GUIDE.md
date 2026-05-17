# Gear5-rs Scraping Operations Guide

The scraper relies on the `gear5-cli` tool. Scrapes can either be executed manually (on-demand) or handled automatically by the API's internal scheduler.

## 1. Manual Scrape (On-Demand)

The manual scrape is best used during local development, or when you want to force an immediate sync of newly released cards without waiting for the nightly cron job.

### Run a Full Scrape
To trigger a scrape of all missing or outdated cards from the official One Piece API, execute the following command from the root of the project:

```bash
# Ensure the DATABASE_URL and the image directory path are set
DATABASE_URL="postgres://gear5:gear5@localhost:5432/gear5" \
GEAR5_IMAGES__DIR="./var/images" \
cargo run -p gear5-cli -- scrape run
```
*(If you are running the optimized release binary, substitute `cargo run -p gear5-cli --` with `./target/release/gear5 scrape run`)*

**What happens when you run this?**
1. The CLI queries the official endpoint for a list of all known sets.
2. It compares the remote sets against your local database.
3. It fetches the cards for any new sets.
4. It downloads the card art (`.png`) and saves them locally into your `GEAR5_IMAGES__DIR` (e.g., `./var/images`).
5. It prints a final status report to the terminal (e.g., `inserted=150 updated=0`).

### View Scrape History
You can view the logs and status of previous scrape attempts (both manual and automated) via the CLI:
```bash
DATABASE_URL="postgres://gear5:gear5@localhost:5432/gear5" \
cargo run -p gear5-cli -- scrape status
```

## 2. Automated Scheduled Scraping (Cron)

The `gear5-api` Axum server has an embedded background scheduler. By default, it is configured to wake up once a day and automatically execute a full scrape to keep your database in sync.

### Configuration
The behavior of the automated scraper is controlled by variables inside your `config.toml` (or via `.env` variables using the `GEAR5_SCRAPE__` prefix):

```toml
[scrape]
enabled = true             # Set to false to disable the background scheduler
cron_hour_utc = 4          # The hour (0-23 UTC) the daily scrape triggers
concurrency = 4            # Number of parallel HTTP requests to the upstream API
stale_after_hours = 36     # How long before the /health/scrape endpoint reports "stale"
```

Because the `gear5-api` is running as a `systemd` background service on your server, **you do not need to do anything manually.** It will automatically perform the scrape at the designated UTC hour every day.

### Checking Scheduler Health
You can externally monitor the health of the background scraper without logging into the server. Make an HTTP request to your live endpoint:
```bash
curl https://gear5.contador.dev/health/scrape
```
This returns a JSON object detailing the `last_run_id`, `last_status`, and whether the data is considered `stale` based on your configured `stale_after_hours`.
