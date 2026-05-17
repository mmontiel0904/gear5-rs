# gear5-rs Agent Instructions

## Workspace Architecture
- `gear5-core`: Shared domain logic, scraper logic, key cryptography, and database queries.
- `gear5-api`: Axum HTTP server and embedded background scrape scheduler.
- `gear5-cli`: Admin CLI (`gear5`) used to run migrations, manual scrapes, and generate API keys.

## Configuration & Environment
- **Env Vars**: Configuration uses `figment` with the `GEAR5_` prefix and double underscores for nesting (e.g., `GEAR5_SCRAPE__CRON_HOUR_UTC`, `GEAR5_DATABASE__MAX_CONNECTIONS`).
- **Database Connection**: Use `DATABASE_URL` directly (e.g., `postgres://gear5:gear5@localhost/gear5`).

## Database & SQLx Quirks (Important)
- **Compile Time**: The codebase deliberately uses `sqlx::query(` instead of the `query!` macro. You do **not** need a running database or `sqlx-data.json` to compile the project or run `cargo clippy`. Do not try to generate offline sqlx data.
- **Migrations**: Always applied via the CLI (`cargo run -p gear5-cli -- migrate`), which reads from `./migrations`.

## Testing
- **Requires DB**: Tests use `#[sqlx::test]`. While you don't need a DB to *compile*, `DATABASE_URL` **must** be set to a running Postgres instance to execute `cargo test`. `sqlx::test` will automatically handle temporary database creation per test.
- **Fixtures**: Scraper tests rely on static HTML fixtures located in `tests/scrape_fixtures/`.
- **Command**: Run a targeted test using: `DATABASE_URL=... cargo test -p <package> --test <filename> <test_name>`

## Dev Commands
- Lint: `cargo clippy --workspace -- -D warnings`
- Boot API: `DATABASE_URL=... cargo run -p gear5-api`
- Issue a new API key: `cargo run -p gear5-cli -- key create --name dev --scopes read,admin`
