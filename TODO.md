# Angel — TODO

## Manual Setup (Charles)

- [ ] Create GCS bucket `charlies-angel` in project `CHIOPS`, region `eu` (multi-region), Standard class, uniform access, public access prevention enforced
- [ ] Create service account `angel@chimera.iam.gserviceaccount.com`, bind Storage Object Admin scoped to `charlies-angel` bucket only
- [ ] Generate SA key JSON → store in macOS Keychain as `angel-gcp-key` (account: `angel@chimera.iam.gserviceaccount.com`, password: full JSON) → delete the file
- [ ] Store Anthropic API key in macOS Keychain as `angel-anthropic-key` (account: `angel`, password: API key string)
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
