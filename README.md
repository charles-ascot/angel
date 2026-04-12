# Angel

Angel is a persistent local macOS application that watches Charles Duckitt working across Claude Code sessions, claude.ai web chats, the Claude desktop app, VSCode workspaces, email, and the local filesystem, and maintains a unified queryable model of what is true *right now* across his entire current workload. Its core promise: Charles will never start work on something that already exists somewhere else without being told, with a clickable deep link to the existing work, before he begins.

## Build instructions

```bash
pnpm install
pnpm tauri dev      # development with hot reload
pnpm tauri build    # production build (.dmg)
```

## Architecture

Angel is a single application internally organised into four modules:

1. **Capture** — reads from sources (Claude Code transcripts, claude.ai, Claude desktop, git, Gmail, filesystem), never reasons, never decides, just persists every artefact to local SQLite before anything else happens.

2. **Classify** — consumes captured artefacts and tags them via the Anthropic API (Claude Opus 4.6). Produces structured records: stream tag, related component, decisions, open questions, status changes, cross-references.

3. **State Model** — SQLite-backed relational structure holding the answer to "what is true right now". Updated incrementally as classified artefacts arrive.

4. **Pushback** — intercepts new Claude Code session starts via filesystem watch, runs similarity checks against the State Model, and surfaces native macOS notifications with clickable deep links to related work.

## Spec

See Notion: CHI-APP-001 — The Angel (link to be added).
