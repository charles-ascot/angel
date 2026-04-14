# Angel — TODO

## Manual Setup (Charles)

- [ ] Create GCS bucket `charlies-angel` in project `CHIOPS`, region `eu` (multi-region), Standard class, uniform access, public access prevention enforced
- [x] Bind `charlies-angel@chiops.iam.gserviceaccount.com` → Storage Object Admin scoped to `charlies-angel` bucket only (not project-wide)
- [x] Generate SA key JSON → store in macOS Keychain as `angel-gcp-key` (account: `charlies-angel@chiops.iam.gserviceaccount.com`, password: full JSON) → delete the file
- [x] Store Anthropic API key in macOS Keychain as `angel-anthropic-key` (account: `angel`, password: API key string)
- [ ] Set git user config in repo (`git config user.name` / `user.email`)
- [ ] Provision Apple Developer ID certificate (defer until first signed release)

## Module Implementation (Claude Code sessions)

- [ ] **Storage** — SQLite schema, GCS async writer
- [ ] **Secrets** — Keychain retrieval wiring
- [ ] **Capture** — filesystem watchers, JSONL parsing, SQLite persistence
- [ ] **Classify** — Anthropic API client, structured tagging
- [ ] **State Model** — relational schema, incremental updates
- [ ] **Pushback** — session-start detection, similarity matching, macOS notifications with deep links
- [ ] **launchd plist** — auto-start at boot, crash restart
- [ ] **Frontend dashboard** — state model viewer in the React UI
