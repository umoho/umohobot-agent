UPDATE threads
SET state = 'draining',
    lease_until = NULL,
    version = version + 1
WHERE state = 'active'
  AND EXISTS (
      SELECT 1
      FROM threads AS newer
      WHERE newer.thread_key = threads.thread_key
        AND newer.state = 'active'
        AND (
            newer.opened_at > threads.opened_at
            OR (newer.opened_at = threads.opened_at AND newer.id > threads.id)
        )
  );

CREATE UNIQUE INDEX IF NOT EXISTS idx_threads_active_thread_key
    ON threads (thread_key)
    WHERE state = 'active';
