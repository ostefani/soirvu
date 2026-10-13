// The editor core's public face for this spike: schema, parse,
// serialize. Phase 2 adds rules.js (input rules), plugins and views
// around the same schema (DESIGN §6.5).

export { schema } from "./schema.js";
export { parseMarkdown, serializeMarkdown } from "./markdown.js";
