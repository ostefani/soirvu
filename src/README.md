# Soirvu frontend (`src/`)

Svelte 5 + plain JavaScript frontend for Soirvu, a local encrypted diary
(Tauri v2). It runs in the webview and does no cryptography itself: every
vault operation goes through the Rust backend, reached only via `api.js`.

## Layout

```
src/
  main.js              entry point; mounts App
  App.svelte           root shell; owns the current screen and switches screens
  api.js               bridge client; the only module that calls Tauri invoke()
  app.css              global styles and theme tokens
  constants.js         app-wide constants
  data.js              shared static data
  assets/              static assets

  containers/          screen-level components: state, data loading, handlers
    MainScreenContainer/       vault list; welcome / locked routing
    LockScreenContainer/       unlock (passphrase, recovery key, Touch ID)
    OnboardingContainer/       first-run vault creation
    DiaryScreenContainer/      unlocked workspace: list, calendar, editor
    SettingsScreenContainer/   settings screen

  components/          views and reusable UI
    Button/ Input/ Select/ Checkbox/ Tabs/          form primitives
    ErrorComponent/ NoteComponent/ WarningComponent/
    Notification/ Description/                      text and feedback
    EntryList/ EntryEditor/ EditorToolbar/ Calendar/
    LockScreen/ Onboarding/ Settings/               screen views

  editor/              hand-built Markdown editor engine (plain JS, no Svelte)
```

Every component and container lives in its own folder. A component folder
holds `index.svelte` and `styles.css`. A container folder additionally
separates its static content from its state:

```
containers/<Name>Container/
  index.svelte    the container: creates $state from the model factory,
                  wires api.js calls and handlers, renders the view
  constants.js    domain constants, the model factory (createModel),
                  field and action configurations
  data.js         UI text only: labels, messages, error and warning text,
                  one exported object per container
  styles.css      (when the container has markup of its own)
```

`constants.js` and `data.js` appear only when the container has static
content to hold — they are not created empty. Constants shared between
containers live in the global `src/constants.js`, not in a container.

## How it fits together

- `App.svelte` holds the screen state. Containers own the logic: they call
  `api.js`, keep `$state`, and pass props down. Components render and report
  events back up through callback props; they never fetch data themselves.
- The backend is the source of truth for vault state (locked/unlocked, vault
  list, settings). The frontend keeps only what the visible screen needs.
- The editor is a separate framework-free engine in `editor/`. The UI uses
  it through `EntryEditor` and `createEditor`. Markdown is the only document
  format: editor state is derived from Markdown on open and serialized back
  on save, never stored.
- Coding rules for this directory: see `CONVENTIONS.md`.
