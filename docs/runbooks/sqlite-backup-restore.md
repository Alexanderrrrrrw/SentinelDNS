# SQLite Backup and Restore

## Backup
1. Stop write-heavy traffic to control plane.
2. Run:
   - `sqlite3 sentinel-config.db ".backup sentinel-config.backup.db"`
3. Store backup in secure storage with timestamp.

## Restore
1. Stop control-plane service.
2. Replace active DB:
   - `copy /Y sentinel-config.backup.db sentinel-config.db`
3. Restart control-plane and verify `/api/devices` response.
