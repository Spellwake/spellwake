# UI journeys with quzzar-flow

The published `quzzar-flow@0.1.0` supports `image`, `text`, `json`, `schema`, and `table`
content blocks in nodes. Use an image block for a static wireframe or screenshot. It accepts SVG files served by a URL or local path, but rejects SVG data URLs. It does
not accept arbitrary HTML or React components inside nodes. Diagram edges describe
navigation; wireframe buttons do not execute navigation. The viewer remains read-only.

The [consumer example](../assets/ui-journey/main.ts) shows SVG wireframes inside three
screen nodes, navigation labels, data contracts, an empty profile, and boundary inputs
that still require user decisions. Its invented sample requirements demonstrate the API;
never transfer them into a real design as approved behavior.

Copy the entire `assets/ui-journey/` directory from this skill into the target project's
ignored `.quzzar-system-design/` workspace. From that copied directory:

```sh
bun install
bun run dev
```

Open the URL Vite prints. Inspect screens and connections to see their data contracts
and processing descriptions. Run `bun run build` to check types and production bundling.

For a screenshot instead of a generated wireframe, use an image block such as:

```ts
{ type: 'image', src: '/screens/profile-empty.png', alt: 'Profile with no records' }
```

That URL must be served by the diagram app, for example from its `public/screens/`
directory. Images are visualization content; contracts still describe the data consumed
and produced by the corresponding screen or state.

Add distinct state nodes for user-agreed failure, empty, large-data, long-text, and upload
boundary behavior. Label arrows with the triggering action or condition, and document
what happens to data at that transition. Keep undecided outcomes explicit in node notes
and scope questions; never invent behavior just to complete a diagram or make validation
pass. Successful contract validation does not establish UX completeness or performance.
