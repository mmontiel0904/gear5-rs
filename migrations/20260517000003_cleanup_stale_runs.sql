-- Mark any scrape_runs rows that were left in 'running' state (e.g. due to a process crash
-- or SIGKILL) as 'failed' so they do not pollute health checks or history queries.
UPDATE scrape_runs
SET status     = 'failed',
    finished_at = COALESCE(finished_at, now()),
    error       = 'process terminated unexpectedly (retroactive cleanup)'
WHERE status = 'running';

UPDATE scrape_run_sets
SET status      = 'failed',
    finished_at = COALESCE(finished_at, now()),
    error       = 'process terminated unexpectedly (retroactive cleanup)'
WHERE status = 'running';
