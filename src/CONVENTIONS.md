# Frontend conventions

Scope: everything under `src/`.

## Language

- Plain JavaScript only. No TypeScript in any form — no `.ts` files, no
  TS-only tooling or config.
- Svelte 5 runes (`$state`, `$props`, `$derived`, `$effect`). No legacy
  Svelte 4 reactivity.
- Single quotes in JavaScript and `<script>` blocks; double quotes in
  markup attributes. Strings containing apostrophes use double quotes.
- No comments in JavaScript or Svelte code. Names carry the meaning.

## Files

- One component per folder: `index.svelte` + `styles.css`, pulled in with
  `<style src="./styles.css"></style>`.
- The folder is named after the component; container folders end in
  `Container`.
- New screens go in `containers/`, reusable UI in `components/`, editor
  internals in `editor/`.

## Containers and components

- Containers own state and side effects: `api.js` calls, `$state` models,
  handlers. Inside a container folder, concerns are split by file:
  `constants.js` holds domain constants, the model factory, and field /
  action configurations; `data.js` holds UI text only; `index.svelte`
  holds the live state and the wiring.
- Data does not live with state: `data.js` is a leaf module — it imports
  nothing and holds no reactive state, so all UI text sits in one place
  and can be lifted into a locale file without touching logic.
- The model factory in `constants.js` returns a plain initial-state
  object; the container wraps it: `let model = $state(createModel())`.
  The factory itself never calls `$state`.
- Naming follows the role: one constants module per container named in
  the singular (`constants.js`), and its exports are `UPPER_SNAKE_CASE`
  constants or a `createModel` factory.
- The pair is created on demand: a container gets a `constants.js` or a
  `data.js` when it has static content to move out of `index.svelte`,
  not by default. Empty placeholder files are not created.
- A constant used by more than one container, or anywhere outside its
  own container, is promoted to the global `src/constants.js`.
  Container-level modules hold only what is local to that container.
- Components are views: props in, callbacks out. A component does not fetch
  data and does not import `api.js`.
- State has one writer: the container. Components receive values as
  one-way props and report changes through callback props (`onchange`,
  `onSaved`, …); the container applies the change to its own `$state`.
  `$bindable` is not used: a child never mutates a prop, so every state
  change traces to an assignment in the container.
- State a component needs only for itself (active tab, an uncommitted
  draft) stays inside the component as its own `$state`; it is not
  lifted into props.
- Primitives (Button, Input, Select, Checkbox, Tabs) take plain props —
  `label`, `name`, `disabled`, `value`/`checked`, `onchange` — and are
  configured at the use site through props and descriptor objects, never
  subclassed or restyled per screen.

## Styling

- Theme values are CSS variables defined in `app.css` (backgrounds, text,
  surfaces, borders, accent, shadows), with light and dark values.
  Component CSS references the variables; it does not define its own
  palette.
- Layout and component rules live in the component's `styles.css`.
  `app.css` holds tokens, base element styles, and app-wide rules only.
- Interactive primitives keep the native element in the DOM — visually
  restyled, not replaced — so keyboard focus and assistive technology
  keep working (see Checkbox).

## Backend access

- `api.js` is the only module that talks to Tauri (`invoke`, event
  listeners). Everything else imports functions from `api.js`.

## Rendering and safety

- `{@html}` is forbidden; `npm run check` enforces it. Diary text renders
  as text nodes. Search snippets are segment arrays rendered with
  `<mark>`, never markup strings.
- The UI is event-driven: no polling and no timer-based synchronization.

## Editor

- `editor/` is plain JavaScript, framework-free. Components use it only
  through `createEditor` from `editor/index.js`.
- Only three npm packages are borrowed: `prosemirror-model`,
  `prosemirror-markdown`, `markdown-it`. Everything else is built
  in-house; adding a dependency needs a reason that survives review.
