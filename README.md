# BottleCapAI Desktop Runner

Connect your local LLMs to BottleCapAI.

## Download

Download the latest release from the [Releases page](https://github.com/limartinyk/bottlecap-runner/releases).

### macOS

After downloading, if you see **"BottleCapAI Runner is damaged and can't be opened"**, run this in Terminal:

```bash
xattr -cr /Applications/BottleCapAI\ Runner.app
```

This removes the quarantine flag that macOS adds to downloaded apps. The app is safe but not yet signed with an Apple Developer certificate.

## Prerequisites

1. **Rust** - Install from [rustup.rs](https://rustup.rs)
2. **Node.js** - v18 or later
3. **Ollama** - Install from [ollama.ai](https://ollama.ai)

## Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev
```

## Building

```bash
# Build for production
npm run tauri build
```

Binaries will be in `src-tauri/target/release/bundle/`.

## Usage

1. Go to [BottleCapAI Dashboard](https://bottlecap.ai/dashboard/runners)
2. Create a new runner and copy the token
3. Paste the token in this app and click Connect
4. Use `local:runner-name/model` in your API calls

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PARTYKIT_URL` | Partykit server URL | `wss://bottlecap-runners.partykit.dev/party/main` |

## Architecture

```
BottleCapAI API
      ↓
  Partykit (WebSocket)
      ↓
  This Desktop App
      ↓
  Local Ollama
```

The app:
1. Connects to Partykit via WebSocket
2. Authenticates with your runner token
3. Reports available Ollama models
4. Receives chat requests from BottleCapAI
5. Forwards them to local Ollama
6. Streams responses back
